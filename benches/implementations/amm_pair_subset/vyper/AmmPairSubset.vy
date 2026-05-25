# pragma version >=0.4.3,<0.5.0

reserve0: public(uint256)
reserve1: public(uint256)
totalLiquidity: public(uint256)

event Mint:
    amount0: uint256
    amount1: uint256
    liquidity: uint256

event Burn:
    liquidity: uint256
    amount0: uint256
    amount1: uint256

event Swap:
    amount0Out: uint256
    amount1Out: uint256
    amount0In: uint256
    amount1In: uint256

event Sync:
    reserve0: uint256
    reserve1: uint256

@external
def mint(amount0: uint256, amount1: uint256) -> uint256:
    assert amount0 > 0 and amount1 > 0, "amount"
    liquidity: uint256 = amount0 + amount1
    self.reserve0 += amount0
    self.reserve1 += amount1
    self.totalLiquidity += liquidity
    log Mint(amount0=amount0, amount1=amount1, liquidity=liquidity)
    return liquidity

@external
def burn(liquidity: uint256) -> (uint256, uint256):
    assert self.totalLiquidity >= liquidity and liquidity > 0, "liquidity"
    amount0: uint256 = self.reserve0 * liquidity // self.totalLiquidity
    amount1: uint256 = self.reserve1 * liquidity // self.totalLiquidity
    self.reserve0 -= amount0
    self.reserve1 -= amount1
    self.totalLiquidity -= liquidity
    log Burn(liquidity=liquidity, amount0=amount0, amount1=amount1)
    return amount0, amount1

@external
def swap(amount0Out: uint256, amount1Out: uint256, amount0In: uint256, amount1In: uint256) -> bool:
    assert amount0Out <= self.reserve0 and amount1Out <= self.reserve1, "reserve"
    assert amount0In > 0 or amount1In > 0, "input"
    self.reserve0 = self.reserve0 + amount0In - amount0Out
    self.reserve1 = self.reserve1 + amount1In - amount1Out
    log Swap(amount0Out=amount0Out, amount1Out=amount1Out, amount0In=amount0In, amount1In=amount1In)
    return True

@external
def sync(balance0: uint256, balance1: uint256) -> bool:
    self.reserve0 = balance0
    self.reserve1 = balance1
    log Sync(reserve0=balance0, reserve1=balance1)
    return True

@external
@view
def getReserves() -> (uint256, uint256):
    return self.reserve0, self.reserve1
