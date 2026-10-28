# pragma version >=0.4.3,<0.5.0

interface SelfPing:
    def ping(x: uint256) -> uint256: view

@external
@pure
def ping(x: uint256) -> uint256:
    return x

@external
@view
def callMany() -> uint256:
    total: uint256 = 0
    for i: uint256 in range({{N}}):
        total += staticcall SelfPing(self).ping(i)
    return total
