# pragma version >=0.4.3,<0.5.0

interface SelfPing:
    def ping(x: uint256): nonpayable

@external
def ping(x: uint256):
    pass

@external
def callMany() -> uint256:
    total: uint256 = 0
    for i: uint256 in range({{N}}):
        extcall SelfPing(self).ping(i)
        total += i
    return total
