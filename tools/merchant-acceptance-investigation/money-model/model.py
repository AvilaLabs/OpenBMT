#!/usr/bin/env python3
"""Synthetic integer-sat merchant liquidity model; no network or node calls."""
from __future__ import annotations

import argparse
import json
from dataclasses import asdict, dataclass


@dataclass(frozen=True)
class Sale:
    at: int
    amount: int


@dataclass(frozen=True)
class Scenario:
    name: str
    sales: tuple[Sale, ...]
    start_cold: int = 1_000_000
    starting_inbound: int = 100_000  # remote, confirmed receive capacity
    hot_cap: int = 100_000
    fee_fixed: int = 3_000
    fee_ppm: int = 1_000
    action_delay: int = 0
    max_pending_exports: int = 1
    swap_available: bool = True
    signer_online: bool = True  # cold signer affects consolidation only; excluded here
    export_threshold: int = 30_000
    min_cold_output: int = 1_000
    future_input_fee_stress: int = 500  # hypothetical per-cold-output liability
    fee_budget: int = 100_000
    max_action_fee: int = 20_000
    withdrawal_batch: int = 0  # 0 = withdraw each tick; n = batch n ticks
    withdrawal_amount: int = 0

    def validate(self) -> None:
        nonnegative = ("start_cold", "starting_inbound", "hot_cap", "fee_fixed",
                      "fee_ppm", "action_delay", "max_pending_exports", "export_threshold", "min_cold_output",
                      "future_input_fee_stress", "fee_budget", "max_action_fee",
                      "withdrawal_batch", "withdrawal_amount")
        for field in nonnegative:
            if getattr(self, field) < 0:
                raise ValueError(f"{self.name}: {field} must be nonnegative")
        if self.hot_cap == 0:
            raise ValueError(f"{self.name}: hot_cap must be positive")
        if self.max_pending_exports == 0:
            raise ValueError(f"{self.name}: max_pending_exports must be positive")
        if self.withdrawal_batch == 1:
            raise ValueError(f"{self.name}: withdrawal_batch must be 0 or >= 2")
        for sale in self.sales:
            if sale.at < 0 or sale.amount <= 0:
                raise ValueError(f"{self.name}: sales need nonnegative ticks and positive amounts")


BASE_SALES = tuple(Sale(t, 8_000) for t in range(12)) + tuple(
    Sale(t, 55_000) for t in (14, 18, 22)
)
SCENARIOS = {
    "small_tickets": Scenario("small_tickets", BASE_SALES),
    "high_fees": Scenario("high_fees", BASE_SALES, fee_fixed=10_000,
                           fee_ppm=5_000, max_action_fee=10_000),
    "signer_offline": Scenario("signer_offline", BASE_SALES, signer_online=False),
    "delayed_action": Scenario("delayed_action", BASE_SALES, action_delay=4,
                               starting_inbound=35_000, export_threshold=10_000),
    "swap_outage": Scenario("swap_outage", BASE_SALES, swap_available=False),
    "low_hot_cap": Scenario("low_hot_cap", tuple(Sale(t, 5_000) for t in range(8)), hot_cap=15_000,
                            starting_inbound=30_000, export_threshold=10_000,
                            fee_fixed=14_000, min_cold_output=3_000,
                            future_input_fee_stress=2_000),
    "withdrawal_batching": Scenario("withdrawal_batching", BASE_SALES,
                                     withdrawal_batch=3, withdrawal_amount=15_000),
    "thousand_small_sales": Scenario(
        "thousand_small_sales", tuple(Sale(t, 1_500) for t in range(1_000)),
        starting_inbound=250_000, hot_cap=250_000, export_threshold=100_000,
        fee_fixed=1_000, future_input_fee_stress=2_000),
    "thousand_sales_slow_settlement": Scenario(
        "thousand_sales_slow_settlement", tuple(Sale(t, 1_500) for t in range(1_000)),
        starting_inbound=250_000, hot_cap=250_000, export_threshold=100_000,
        fee_fixed=1_000, action_delay=120, future_input_fee_stress=2_000),
    "thousand_sales_pipelined": Scenario(
        "thousand_sales_pipelined", tuple(Sale(t, 1_500) for t in range(1_000)),
        starting_inbound=250_000, hot_cap=250_000, export_threshold=50_000,
        fee_fixed=1_000, action_delay=120, max_pending_exports=4,
        future_input_fee_stress=2_000),
    "thousand_sales_small_serial_batches": Scenario(
        "thousand_sales_small_serial_batches", tuple(Sale(t, 1_500) for t in range(1_000)),
        starting_inbound=250_000, hot_cap=250_000, export_threshold=50_000,
        fee_fixed=1_000, action_delay=120, max_pending_exports=1,
        future_input_fee_stress=2_000),
}
POLICIES = ("onchain", "hold", "splice", "swapbatch")


class Simulation:
    """Serial ticks; any action due at tick t confirms before t's sale events."""

    def __init__(self, scenario: Scenario, policy: str):
        scenario.validate()
        if policy not in POLICIES:
            raise ValueError(f"unknown policy: {policy}")
        self.s, self.policy = scenario, policy
        self.i = 0 if policy == "onchain" else scenario.starting_inbound
        self.capacity = self.i
        self.l = 0
        self.cold = scenario.start_cold
        self.fees_paid = self.fees_reserved = 0
        self.cold_outputs: list[int] = []
        self.uneconomic_outputs = 0
        self.uneconomic_export_blocks = 0
        self.sales_seen = self.sales_paid = self.sales_refused = 0
        self.recognized_revenue = 0
        self.refusals: dict[str, int] = {}
        self.withdrawn = self.withdrawal_unavailable = 0
        self.withdrawal_batch_value = 0
        self.withdrawal_due: int | None = None
        self.pending_actions: list[dict] = []
        self.events: list[dict] = []

    def action_fee(self, amount: int) -> int:
        return self.s.fee_fixed + (amount * self.s.fee_ppm + 999_999) // 1_000_000

    def log(self, t: int, kind: str, **details: object) -> None:
        self.events.append({"t": t, "kind": kind, "inbound": self.i,
                            "hot": self.l, "capacity": self.capacity,
                            "cold": self.cold, "fees_paid": self.fees_paid,
                            "fees_reserved": self.fees_reserved,
                            "pending_export_count": len(self.pending_actions),
                            "pending_export_principal": self.reserved_principal,
                            **details})
        self.assert_state()

    @property
    def reserved_principal(self) -> int:
        return sum(action["amount"] for action in self.pending_actions)

    def assert_state(self) -> None:
        assert self.i >= 0 and self.l >= 0 and self.cold >= 0
        assert self.capacity == self.i + self.l
        assert self.l <= self.s.hot_cap
        assert self.fees_paid + self.fees_reserved <= self.s.fee_budget
        assert self.fees_reserved == sum(action["fee"] for action in self.pending_actions)
        assert self.reserved_principal <= self.l
        assert len(self.pending_actions) <= self.s.max_pending_exports
        assert self.cold + self.l - self.s.start_cold == (
            self.recognized_revenue - self.withdrawn - self.fees_paid
        )

    def refuse(self, t: int, amount: int, reason: str) -> None:
        self.sales_refused += 1
        self.refusals[reason] = self.refusals.get(reason, 0) + 1
        self.log(t, "sale_refused_unpaid", amount=amount, reason=reason)

    def output(self, amount: int, source: str) -> None:
        # A completed on-chain receipt creates a cold output with no assumed
        # merchant receive fee. Hot-wallet exports use amount less their fee.
        self.cold += amount
        self.cold_outputs.append(amount)
        if amount <= self.s.future_input_fee_stress:
            self.uneconomic_outputs += 1
        self.log(self.now, "cold_output_created", value=amount, source=source,
                 future_input_fee_stress=self.s.future_input_fee_stress)

    def finish_action(self, t: int) -> None:
        due = [action for action in self.pending_actions if action["due"] <= t]
        for p in due:
            x, fee = p["amount"], p["fee"]
            # Fee was reserved at start and is paid once from the export. Until
            # confirmation x stays in L and cannot fund ordinary payments.
            self.l -= x
            if p["kind"] == "swapbatch":
                self.i += x
            else:  # spliceout consumes channel capacity, without replenishing I
                self.capacity -= x
            self.fees_reserved -= fee
            self.fees_paid += fee
            self.pending_actions.remove(p)
            self.output(x - fee, p["kind"])
            self.log(t, f"{p['kind']}_confirmed", amount=x, fee=fee, output=x-fee)

    def begin_export(self, t: int) -> None:
        if self.policy not in ("splice", "swapbatch"):
            return
        if len(self.pending_actions) >= self.s.max_pending_exports:
            return
        x = self.l - self.reserved_principal
        if x < self.s.export_threshold:
            return
        if self.policy == "swapbatch" and not self.s.swap_available:
            self.log(t, "action_blocked", action="swapbatch", reason="service_unavailable")
            return
        fee = self.action_fee(x)
        output = x - fee
        if output < self.s.min_cold_output or output <= self.s.future_input_fee_stress:
            self.uneconomic_export_blocks += 1
            self.log(t, "action_blocked", action=self.policy,
                     reason="uneconomic_output", amount=x, fee=fee, output=output)
            return
        if self.fees_paid + self.fees_reserved + fee > self.s.fee_budget:
            self.log(t, "action_blocked", action=self.policy, reason="fee_budget", fee=fee)
            return
        if fee > self.s.max_action_fee:
            self.log(t, "action_blocked", action=self.policy,
                     reason="action_fee_limit", fee=fee, limit=self.s.max_action_fee)
            return
        self.fees_reserved += fee
        self.pending_actions.append({"kind": self.policy, "amount": x, "fee": fee,
                                     "due": t + self.s.action_delay})
        self.log(t, f"{self.policy}_started", amount=x, fee=fee, due=t+self.s.action_delay)
        if self.s.action_delay == 0:
            self.finish_action(t)

    def receive_sale(self, t: int, amount: int) -> None:
        self.sales_seen += 1
        if self.policy == "onchain":
            self.sales_paid += 1
            self.recognized_revenue += amount
            self.output(amount, "onchain_sale")
            self.log(t, "sale_paid_onchain", amount=amount, receive_fee=0)
            return
        if amount > self.i:
            self.refuse(t, amount, "insufficient_confirmed_inbound")
            return
        if amount > self.s.hot_cap - self.l:
            self.refuse(t, amount, "hot_capacity")
            return
        self.i -= amount
        self.l += amount
        self.sales_paid += 1
        self.recognized_revenue += amount
        self.log(t, "sale_paid_lightning", amount=amount)
        self.begin_export(t)
        self.assert_state()

    def pay_withdrawal(self, t: int, amount: int) -> None:
        available = self.l - self.reserved_principal
        if amount > available:
            self.withdrawal_unavailable += 1
            self.log(t, "withdrawal_unavailable", amount=amount,
                     available_hot=available)
            return
        self.l -= amount
        self.i += amount
        self.withdrawn += amount
        self.log(t, "withdrawal_paid_lightning", amount=amount)
        self.assert_state()

    def withdrawal_tick(self, t: int) -> None:
        s = self.s
        if not s.withdrawal_amount or self.policy == "onchain":
            return
        if s.withdrawal_batch:
            self.withdrawal_batch_value += s.withdrawal_amount
            if self.withdrawal_due is None:
                self.withdrawal_due = t + s.withdrawal_batch - 1
            if t >= self.withdrawal_due:
                amount = self.withdrawal_batch_value
                self.withdrawal_batch_value = 0
                self.withdrawal_due = None
                self.pay_withdrawal(t, amount)
            else:
                self.log(t, "withdrawal_batched", amount=s.withdrawal_amount,
                         pending=self.withdrawal_batch_value, due=self.withdrawal_due)
        else:
            self.pay_withdrawal(t, s.withdrawal_amount)

    def run(self) -> dict:
        last = max((sale.at for sale in self.s.sales), default=-1)
        last = max(last, 0 if self.s.withdrawal_amount else -1)
        sales_by_tick: dict[int, list[Sale]] = {}
        for sale in self.s.sales:
            sales_by_tick.setdefault(sale.at, []).append(sale)
        for t in range(last + 1):
            self.now = t
            self.finish_action(t)
            for sale in sales_by_tick.get(t, ()):
                self.receive_sale(t, sale.amount)
            self.withdrawal_tick(t)
            self.assert_state()
        own_now = self.cold + self.l
        own_initial = self.s.start_cold
        expected = self.recognized_revenue - self.withdrawn - self.fees_paid
        assert own_now - own_initial == expected
        return {
            "scenario": self.s.name, "policy": self.policy,
            "sales_seen": self.sales_seen, "sales_paid": self.sales_paid,
            "sales_refused_unpaid": self.sales_refused,
            "refusals_by_reason": self.refusals,
            "recognized_revenue": self.recognized_revenue,
            "withdrawn": self.withdrawn,
            "withdrawal_unavailable": self.withdrawal_unavailable,
            "fees_paid": self.fees_paid, "fees_reserved_pending": self.fees_reserved,
            "peak_hot": max((event["hot"] for event in self.events), default=0),
            "cold_output_count": len(self.cold_outputs),
            "cold_output_values": self.cold_outputs,
            "cold_output_total": sum(self.cold_outputs),
            "aggregate_future_input_fee_stress": len(self.cold_outputs) * self.s.future_input_fee_stress,
            "uneconomic_cold_output_count": self.uneconomic_outputs,
            "uneconomic_export_block_count": self.uneconomic_export_blocks,
            "own_capital_initial_cold_plus_hot": own_initial,
            "own_capital_ending_cold_plus_hot": own_now,
            "own_capital_net_change": own_now - own_initial,
            "external_confirmed_remote_inbound_capacity_initial":
                None if self.policy == "onchain" else self.s.starting_inbound,
            "ending": {"inbound": self.i, "hot": self.l, "capacity": self.capacity,
                       "cold": self.cold,
                       "pending_action": (self.pending_actions[0] if len(self.pending_actions) == 1
                                          else (self.pending_actions or None)),
                       "pending_actions": self.pending_actions,
                       "pending_withdrawal": self.withdrawal_batch_value},
            "events": self.events,
        }


def scenario_json(s: Scenario) -> dict:
    return asdict(s)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=SCENARIOS, default="small_tickets")
    parser.add_argument("--policy", choices=POLICIES, default="swapbatch")
    parser.add_argument("--all", action="store_true", help="all scenarios and policies")
    args = parser.parse_args()
    names = tuple(SCENARIOS) if args.all else (args.scenario,)
    policies = POLICIES if args.all else (args.policy,)
    output = {"model": "synthetic_integer_sat_feasibility_v2",
              "inputs": {n: scenario_json(SCENARIOS[n]) for n in names},
              "results": [Simulation(SCENARIOS[n], policy).run()
                          for n in names for policy in policies]}
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
