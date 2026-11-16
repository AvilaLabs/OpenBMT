import unittest

from model import Sale, Scenario, Simulation


class ModelTests(unittest.TestCase):
    def test_swap_pending_keeps_hot_exposure_and_does_not_restore_inbound(self):
        s = Scenario("delay", (Sale(0, 40_000), Sale(1, 15_000), Sale(2, 100_000)),
                     starting_inbound=50_000, hot_cap=60_000,
                     export_threshold=30_000, action_delay=2,
                     fee_fixed=1_000, fee_ppm=0, max_action_fee=2_000)
        r = Simulation(s, "swapbatch").run()
        started = next(e for e in r["events"] if e["kind"] == "swapbatch_started")
        self.assertEqual((started["inbound"], started["hot"], started["capacity"]),
                         (10_000, 40_000, 50_000))
        self.assertEqual(r["refusals_by_reason"], {"insufficient_confirmed_inbound": 2})
        self.assertEqual(r["ending"]["inbound"], 50_000)
        self.assertEqual(r["ending"]["hot"], 0)
        self.assertEqual(r["fees_paid"], 1_000)
        self.assertEqual(r["own_capital_net_change"], 39_000)

    def test_hot_export_fee_is_paid_from_output_once(self):
        s = Scenario("cashflow", (Sale(0, 30_000),), starting_inbound=30_000,
                     hot_cap=40_000, export_threshold=10_000,
                     fee_fixed=500, fee_ppm=0, max_action_fee=500)
        r = Simulation(s, "swapbatch").run()
        self.assertEqual(r["cold_output_values"], [29_500])
        self.assertEqual(r["fees_paid"], 500)
        self.assertEqual(r["own_capital_net_change"], 29_500)
        self.assertEqual(r["ending"]["capacity"],
                         r["ending"]["inbound"] + r["ending"]["hot"])

    def test_onchain_receipt_is_full_value_one_output_and_payer_fee_excluded(self):
        s = Scenario("chain", (Sale(0, 900), Sale(1, 2_000)),
                     future_input_fee_stress=1_000)
        r = Simulation(s, "onchain").run()
        self.assertEqual(r["recognized_revenue"], 2_900)
        self.assertEqual(r["fees_paid"], 0)
        self.assertEqual(r["cold_output_values"], [900, 2_000])
        self.assertEqual(r["uneconomic_cold_output_count"], 1)
        self.assertEqual(r["aggregate_future_input_fee_stress"], 2_000)

    def test_uneconomic_export_does_not_assume_extra_hot_buffer(self):
        s = Scenario("too-small", tuple(Sale(t, 5_000) for t in range(4)),
                     starting_inbound=20_000, hot_cap=15_000,
                     export_threshold=10_000, fee_fixed=14_000,
                     fee_ppm=0, min_cold_output=3_000,
                     future_input_fee_stress=2_000, max_action_fee=14_000)
        r = Simulation(s, "swapbatch").run()
        self.assertEqual(r["sales_paid"], 3)
        self.assertEqual(r["recognized_revenue"], 15_000)
        self.assertEqual(r["refusals_by_reason"], {"hot_capacity": 1})
        self.assertGreater(r["uneconomic_export_block_count"], 0)
        self.assertTrue(any(e.get("reason") == "uneconomic_output" for e in r["events"]))

    def test_signer_offline_does_not_block_hot_key_swap(self):
        s = Scenario("offline", (Sale(0, 40_000),), starting_inbound=40_000,
                     hot_cap=50_000, export_threshold=10_000, signer_online=False,
                     fee_fixed=1_000, fee_ppm=0, max_action_fee=1_000)
        r = Simulation(s, "swapbatch").run()
        self.assertEqual(r["sales_paid"], 1)
        self.assertEqual(r["cold_output_count"], 1)
        self.assertEqual(r["ending"]["pending_action"], None)

    def test_receipt_equal_to_its_future_input_cost_has_no_net_value(self):
        r = Simulation(Scenario("break-even", (Sale(0, 1_000),),
                                future_input_fee_stress=1_000), "onchain").run()
        self.assertEqual(r["recognized_revenue"], 1_000)
        self.assertEqual(r["uneconomic_cold_output_count"], 1)

    def test_reserved_pending_hot_value_cannot_fund_withdrawal(self):
        s = Scenario("reserved", (Sale(0, 40_000), Sale(1, 1_000)),
                     starting_inbound=50_000, hot_cap=50_000,
                     export_threshold=30_000, action_delay=2,
                     fee_fixed=1_000, fee_ppm=0, max_action_fee=1_000,
                     withdrawal_amount=5_000)
        r = Simulation(s, "swapbatch").run()
        self.assertEqual(r["withdrawal_unavailable"], 2)
        self.assertEqual(r["fees_paid"], 0)
        self.assertEqual(r["fees_reserved_pending"], 1_000)
        self.assertEqual(r["ending"]["hot"], 41_000)

    def test_bounded_pipeline_reserves_principal_without_early_capacity_or_spend(self):
        s = Scenario("pipeline", (Sale(0, 10_000), Sale(1, 10_000),
                                   Sale(2, 25_000)),
                     starting_inbound=40_000, hot_cap=40_000,
                     export_threshold=10_000, action_delay=3,
                     max_pending_exports=2, fee_fixed=100, fee_ppm=0,
                     max_action_fee=100, withdrawal_amount=5_000)
        r = Simulation(s, "swapbatch").run()
        starts = [e for e in r["events"] if e["kind"] == "swapbatch_started"]
        self.assertEqual(len(starts), 2)
        self.assertEqual(starts[0]["inbound"], 30_000)
        self.assertEqual(starts[1]["inbound"], 20_000)
        self.assertEqual(starts[1]["pending_export_principal"], 20_000)
        self.assertEqual(r["refusals_by_reason"], {"insufficient_confirmed_inbound": 1})
        self.assertEqual(r["withdrawal_unavailable"], 3)
        self.assertEqual(r["ending"]["inbound"], 20_000)
        self.assertEqual(r["ending"]["hot"], 20_000)
        self.assertEqual(len(r["ending"]["pending_actions"]), 2)
        self.assertEqual(r["fees_paid"], 0)
        self.assertEqual(r["fees_reserved_pending"], 200)

    def test_invalid_runtime_inputs_are_rejected(self):
        with self.assertRaises(ValueError):
            Simulation(Scenario("bad", (Sale(0, 1),), hot_cap=-1), "hold")
        with self.assertRaises(ValueError):
            Simulation(Scenario("bad-tick", (Sale(-1, 1),)), "hold")
        with self.assertRaises(ValueError):
            Simulation(Scenario("bad-policy", (Sale(0, 1),)), "jit")


if __name__ == "__main__":
    unittest.main()
