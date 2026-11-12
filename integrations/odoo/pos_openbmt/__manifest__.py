# Checked against: odoo/odoo 18.0 (run in a real server): the manifest keys and
# the `assets` bucket point_of_sale._assets_pos as in addons/pos_paytm/
# __manifest__.py (lines 5-28), the web.assets_tests bucket as in
# addons/point_of_sale/__manifest__.py (line 83). The license key is the MIT
# text in LICENSE.
{
    'name': 'Point of Sale OpenBMT',
    'version': '18.0.1.0.0',
    'category': 'Sales/Point of Sale',
    'summary': 'Take bitcoin at the till through an OpenBMT Box',
    'depends': ['point_of_sale'],
    'data': [
        'views/pos_payment_method_views.xml',
    ],
    'assets': {
        'point_of_sale._assets_pos': [
            'pos_openbmt/static/src/**/*',
        ],
        'web.assets_tests': [
            'pos_openbmt/static/tests/tours/**/*',
        ],
    },
    'installable': True,
    'license': 'Other OSI approved licence',
}
