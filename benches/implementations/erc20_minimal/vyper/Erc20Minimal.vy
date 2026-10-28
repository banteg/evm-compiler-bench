# pragma version >=0.4.3,<0.5.0

NAME: constant(String[11]) = "Bench Token"
SYMBOL: constant(String[5]) = "BENCH"
DECIMALS: constant(uint8) = 18

totalSupply: public(uint256)
balanceOf: public(HashMap[address, uint256])
allowance: public(HashMap[address, HashMap[address, uint256]])

event Transfer:
    sender: indexed(address)
    receiver: indexed(address)
    value: uint256

event Approval:
    owner: indexed(address)
    spender: indexed(address)
    value: uint256

@deploy
def __init__(initialSupply: uint256):
    self.totalSupply = initialSupply
    self.balanceOf[msg.sender] = initialSupply
    log Transfer(sender=empty(address), receiver=msg.sender, value=initialSupply)

@external
@pure
def name() -> String[11]:
    return NAME

@external
@pure
def symbol() -> String[5]:
    return SYMBOL

@external
@pure
def decimals() -> uint8:
    return DECIMALS

@external
def transfer(to: address, amount: uint256) -> bool:
    assert self.balanceOf[msg.sender] >= amount, "balance"
    self.balanceOf[msg.sender] -= amount
    self.balanceOf[to] += amount
    log Transfer(sender=msg.sender, receiver=to, value=amount)
    return True

@external
def approve(spender: address, amount: uint256) -> bool:
    self.allowance[msg.sender][spender] = amount
    log Approval(owner=msg.sender, spender=spender, value=amount)
    return True

@external
def transferFrom(sender: address, to: address, amount: uint256) -> bool:
    allowed: uint256 = self.allowance[sender][msg.sender]
    assert allowed >= amount, "allowance"
    assert self.balanceOf[sender] >= amount, "balance"
    self.allowance[sender][msg.sender] = allowed - amount
    self.balanceOf[sender] -= amount
    self.balanceOf[to] += amount
    log Transfer(sender=sender, receiver=to, value=amount)
    return True
