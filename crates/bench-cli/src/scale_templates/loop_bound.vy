# pragma version >=0.4.3,<0.5.0

@external
@pure
def runLoop() -> uint256:
    total: uint256 = 0
    for i: uint256 in range({{N}}):
        total += i
    return total
