# pragma version >=0.4.3,<0.5.0

event Tick:
    index: indexed(uint256)
    value: uint256

@external
def emitMany() -> uint256:
    total: uint256 = 0
    for i: uint256 in range({{N}}):
        log Tick(index=i, value=i + 1)
        total += i + 1
    return total
