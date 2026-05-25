# pragma version >=0.4.3,<0.5.0

@external
@view
def ping(x: uint256):
    pass

@external
def callMany() -> uint256:
    total: uint256 = 0
    for i: uint256 in range({{N}}):
        raw_call(self, concat(method_id("ping(uint256)"), abi_encode(i)), max_outsize=0, is_static_call=True)
        total += i
    return total
