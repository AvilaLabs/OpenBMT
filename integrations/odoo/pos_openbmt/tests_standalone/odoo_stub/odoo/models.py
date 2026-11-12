class Model:
    """One record; keyword arguments set its fields."""

    def __init__(self, **values):
        for name, value in values.items():
            setattr(self, name, value)

    def ensure_one(self):
        return self

    def sudo(self):
        return self

    def __iter__(self):
        return iter([self])

    def _get_payment_terminal_selection(self):
        return [('other', 'Other')]
