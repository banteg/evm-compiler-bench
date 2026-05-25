# pragma version >=0.4.3,<0.5.0

owner: public(address)
paused: public(bool)
counter: public(uint256)

event OwnershipTransferred:
    previousOwner: indexed(address)
    newOwner: indexed(address)

event Paused:
    account: indexed(address)

event Unpaused:
    account: indexed(address)

@deploy
def __init__():
    self.owner = msg.sender
    log OwnershipTransferred(previousOwner=empty(address), newOwner=msg.sender)

@internal
@view
def _only_owner():
    assert msg.sender == self.owner, "owner"

@external
def transferOwnership(newOwner: address) -> bool:
    self._only_owner()
    assert newOwner != empty(address), "zero"
    log OwnershipTransferred(previousOwner=self.owner, newOwner=newOwner)
    self.owner = newOwner
    return True

@external
def pause() -> bool:
    self._only_owner()
    self.paused = True
    log Paused(account=msg.sender)
    return True

@external
def unpause() -> bool:
    self._only_owner()
    self.paused = False
    log Unpaused(account=msg.sender)
    return True

@external
def guardedIncrement() -> uint256:
    assert not self.paused, "paused"
    self.counter += 1
    return self.counter
