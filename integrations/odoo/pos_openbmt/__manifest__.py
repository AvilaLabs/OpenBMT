# Checked against: odoo/odoo 18.0, addons/pos_paytm/__manifest__.py (as quoted
# in the research notes). The license key is the MIT text in LICENSE.
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
    },
    'installable': True,
    'license': 'Other OSI approved licence',
}
