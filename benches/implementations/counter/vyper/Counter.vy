# pragma version >=0.4.3,<0.5.0

value: public(uint256)

@deploy
def __init__(initialValue: uint256):
    self.value = initialValue

@external
def increment() -> uint256:
    self.value += 1
    return self.value

@external
def add(amount: uint256) -> uint256:
    self.value += amount
    return self.value

@external
def reset() -> uint256:
    self.value = 0
    return self.value
