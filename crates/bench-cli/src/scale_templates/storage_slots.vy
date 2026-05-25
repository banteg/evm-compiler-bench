# pragma version >=0.4.3,<0.6.0

{{SLOTS}}

@external
def writeAll(seed: uint256) -> uint256:
    total: uint256 = 0
{{WRITE_BODY}}
    return total

@external
@view
def readAll() -> uint256:
    total: uint256 = 0
{{READ_BODY}}
    return total
