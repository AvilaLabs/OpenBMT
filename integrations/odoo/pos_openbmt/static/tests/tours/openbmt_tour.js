import * as Chrome from "@point_of_sale/../tests/tours/utils/chrome_util";
import * as ProductScreen from "@point_of_sale/../tests/tours/utils/product_screen_util";
import * as PaymentScreen from "@point_of_sale/../tests/tours/utils/payment_screen_util";
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("OpenBMTPaymentTour", {
    steps: () =>
        [
            Chrome.startPoS(),
            {
                content: "open the register",
                trigger: ".modal-footer .btn:contains('Open Register')",
                run: "click",
            },
            ProductScreen.addOrderline("Bitcoin Pen", "1"),
            ProductScreen.clickPayButton(),
            PaymentScreen.clickPaymentMethod("Bitcoin"),
            {
                content: "the line waits for the Send button",
                trigger: ".paymentline.electronic_payment .electronic_status:contains('pending')",
            },
            {
                content: "send the sale to the Box",
                trigger: ".paymentline.electronic_payment .send_payment_request",
                run: "click",
            },
            {
                content: "the Box reports the sale as paid and the order is validated",
                trigger: ".pos .receipt-screen",
                timeout: 30000,
            },
        ].flat(),
});
