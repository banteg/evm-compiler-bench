# pragma version >=0.4.3,<0.5.0

balanceOf: public(HashMap[address, uint256])
totalShares: public(uint256)

event Deposit:
    account: indexed(address)
    amount: uint256

event Withdraw:
    account: indexed(address)
    amount: uint256

@external
@payable
def deposit() -> uint256:
    assert msg.value > 0, "value"
    self.balanceOf[msg.sender] += msg.value
    self.totalShares += msg.value
    log Deposit(account=msg.sender, amount=msg.value)
    return msg.value

@external
def withdraw(amount: uint256) -> uint256:
    assert self.balanceOf[msg.sender] >= amount, "shares"
    self.balanceOf[msg.sender] -= amount
    self.totalShares -= amount
    raw_call(msg.sender, b"", value=amount)
    log Withdraw(account=msg.sender, amount=amount)
    return amount

@external
@view
def totalAssets() -> uint256:
    return self.balance
