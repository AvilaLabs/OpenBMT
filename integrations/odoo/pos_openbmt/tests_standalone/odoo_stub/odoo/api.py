def constrains(*names):
    def wrap(func):
        func._constrains = names
        return func
    return wrap
