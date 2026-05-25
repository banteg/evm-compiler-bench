# pragma version >=0.4.3,<0.6.0

sink: public(uint256)

@external
def setSink(new_value: uint256) -> uint256:
    self.sink = new_value
    return new_value

{{FUNCTIONS}}
