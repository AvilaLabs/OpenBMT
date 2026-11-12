/* Checked against: odoo/odoo 18.0 (branch 18.0, run in a real server and
   browser): point_of_sale/static/src/app/payment/payment_interface.js (class
   and the setup, fast_payments, send_payment_request and send_payment_cancel
   hooks), point_of_sale/static/src/app/store/pos_store.js
   register_payment_method (export, near line 2439) and `this.currency =
   this.config.currency_id` (line 311), point_of_sale/static/src/app/models/
   data_service.js silentCall(model, method, args, kwargs, queue) (line 622;
   it returns false when the call fails), point_of_sale/static/src/app/
   models/pos_payment.js pay() (line 73), point_of_sale/static/src/app/
   models/pos_order.js payment_ids and get_selected_paymentline (line 581),
   web/static/src/core/confirmation_dialog/confirmation_dialog.js AlertDialog
   (line 93), res.currency fields decimal_places and name loaded by
   point_of_sale/models/res_currency.py (line 17). The tour in
   tests/test_openbmt_tour.py runs the whole path. */
import { PaymentInterface } from "@point_of_sale/app/payment/payment_interface";
import { register_payment_method } from "@point_of_sale/app/store/pos_store";
import { AlertDialog } from "@web/core/confirmation_dialog/confirmation_dialog";

const POLL_MS = 2000;
const TEXT = {
    tillWaiting: "Waiting for the counter. Press Start on the counter.",
    tillOpened: "The customer can pay now.",
    tillServer: {
        heading: "The till lost its connection to Odoo",
        what: "The till could not reach Odoo's server to ask the Box.",
        next: "Check the connection, then press Retry. If the customer may have paid already, look at the counter first.",
    },
};

export class PaymentOpenBMT extends PaymentInterface {
    setup() {
        super.setup(...arguments);
        this.supports_reversals = false;
        this.sale = null;
        this.stopped = false;
        this.timer = null;
        this.wake = null;
    }

    get fast_payments() {
        return false;
    }

    async _call(method, args) {
        const result = await this.pos.data.silentCall("pos.payment.method", method, [
            [this.payment_method_id.id],
            ...args,
        ]);
        // silentCall answers false when Odoo's own server cannot be reached.
        return result || { error: "tillServer", words: TEXT.tillServer };
    }

    _line(uuid) {
        const order = this.pos.get_order();
        return order.payment_ids.find((l) => l.uuid === uuid) || order.get_selected_paymentline();
    }

    _sleep(ms) {
        return new Promise((resolve) => {
            this.wake = resolve;
            this.timer = setTimeout(() => {
                this.timer = null;
                this.wake = null;
                resolve();
            }, ms);
        });
    }

    _stop() {
        this.stopped = true;
        if (this.timer !== null) {
            clearTimeout(this.timer);
            this.timer = null;
        }
        if (this.wake) {
            const wake = this.wake;
            this.wake = null;
            wake();
        }
    }

    _tell(text) {
        this.env.services.notification.add(text, { type: "info" });
    }

    _fail(line, result) {
        const words = result && result.words;
        const body = words
            ? [words.what, words.next].filter((t) => t).join("\n")
            : (result && result.error) || "";
        this.env.services.dialog.add(AlertDialog, {
            title: words ? words.heading : (result && result.error) || "",
            body,
        });
        line.set_payment_status("retry");
        return false;
    }

    async send_payment_request(uuid) {
        await super.send_payment_request(uuid);
        const line = this._line(uuid);
        const order = this.pos.get_order();
        this.stopped = false;
        this.sale = null;
        line.set_payment_status("waitingCard");
        const amount = line.amount.toFixed(this.pos.currency.decimal_places);
        let result = await this._call("openbmt_start", [
            amount,
            this.pos.currency.name,
            order.name,
            uuid,
        ]);
        if (this.stopped) {
            return false;
        }
        let shown = null;
        for (;;) {
            if (result.error) {
                return this._fail(line, result);
            }
            this.sale = result.sale;
            if (result.state === "paid") {
                line.transaction_id = result.sale;
                return true;
            }
            if (result.state !== "waiting" && result.state !== "opened") {
                return this._fail(line, result);
            }
            if (result.state !== shown) {
                shown = result.state;
                this._tell(result.state === "opened" ? TEXT.tillOpened : TEXT.tillWaiting);
            }
            await this._sleep(POLL_MS);
            if (this.stopped) {
                return false;
            }
            result = await this._call("openbmt_status", [this.sale]);
            if (this.stopped) {
                return false;
            }
        }
    }

    async send_payment_cancel(order, uuid) {
        await super.send_payment_cancel(order, uuid);
        const sale = this.sale;
        this._stop();
        if (sale) {
            await this._call("openbmt_cancel", [sale]);
        }
        return true;
    }
}

register_payment_method("openbmt", PaymentOpenBMT);
