# pragma version >=0.4.3,<0.6.0

{{LINKS}}

@external
def writeChain(seed: uint256) -> uint256:
    current: uint256 = seed
{{WRITE_BODY}}
    return current

@external
@view
def readChain(seed: uint256) -> uint256:
    current: uint256 = seed
{{READ_BODY}}
    return current
