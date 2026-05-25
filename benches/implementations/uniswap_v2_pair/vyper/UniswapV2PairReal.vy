# pragma version >=0.4.3,<0.5.0

interface ERC20:
    def balanceOf(owner: address) -> uint256: view
    def transfer(receiver: address, amount: uint256) -> bool: nonpayable

interface Factory:
    def feeTo() -> address: view

interface UniswapV2Callee:
    def uniswapV2Call(sender: address, amount0: uint256, amount1: uint256, data: Bytes[65536]): nonpayable

MINIMUM_LIQUIDITY: public(constant(uint256)) = 1000
Q112: constant(uint256) = 5192296858534827628530496329220096
DOMAIN_TYPEHASH: constant(bytes32) = keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
NAME_HASH: constant(bytes32) = keccak256("Uniswap V2")
VERSION_HASH: constant(bytes32) = keccak256("1")
PERMIT_TYPEHASH_VALUE: constant(bytes32) = 0x6e71edae12b1b97f4d1f60370fef10105fa2faae0126114a169c64845d6126c9

factory: public(address)
token0: public(address)
token1: public(address)
reserve0: uint112
reserve1: uint112
blockTimestampLast: uint32
price0CumulativeLast: public(uint256)
price1CumulativeLast: public(uint256)
kLast: public(uint256)
totalSupply: public(uint256)
balanceOf: public(HashMap[address, uint256])
allowance: public(HashMap[address, HashMap[address, uint256]])
DOMAIN_SEPARATOR: public(bytes32)
nonces: public(HashMap[address, uint256])
unlocked: bool

event Approval:
    owner: indexed(address)
    spender: indexed(address)
    amount: uint256

event Transfer:
    sender: indexed(address)
    receiver: indexed(address)
    amount: uint256

event Mint:
    sender: indexed(address)
    amount0: uint256
    amount1: uint256

event Burn:
    sender: indexed(address)
    amount0: uint256
    amount1: uint256
    receiver: indexed(address)

event Swap:
    sender: indexed(address)
    amount0In: uint256
    amount1In: uint256
    amount0Out: uint256
    amount1Out: uint256
    receiver: indexed(address)

event Sync:
    reserve0: uint112
    reserve1: uint112

@deploy
def __init__():
    self.factory = msg.sender
    self.DOMAIN_SEPARATOR = keccak256(abi_encode(DOMAIN_TYPEHASH, NAME_HASH, VERSION_HASH, chain.id, self))
    self.unlocked = True

@external
@view
def name() -> String[10]:
    return "Uniswap V2"

@external
@view
def symbol() -> String[6]:
    return "UNI-V2"

@external
@view
def decimals() -> uint8:
    return 18

@external
@view
def PERMIT_TYPEHASH() -> bytes32:
    return PERMIT_TYPEHASH_VALUE

@external
def initialize(token0_: address, token1_: address):
    assert msg.sender == self.factory, "UniswapV2: FORBIDDEN"
    self.token0 = token0_
    self.token1 = token1_

@external
def approve(spender: address, amount: uint256) -> bool:
    self.allowance[msg.sender][spender] = amount
    log Approval(owner=msg.sender, spender=spender, amount=amount)
    return True

@external
def permit(owner: address, spender: address, amount: uint256, deadline: uint256, v: uint8, r: bytes32, s: bytes32):
    assert deadline >= block.timestamp, "UniswapV2: EXPIRED"
    nonce: uint256 = self.nonces[owner]
    self.nonces[owner] = unsafe_add(nonce, 1)
    digest: bytes32 = keccak256(
        concat(
            b"\x19\x01",
            self.DOMAIN_SEPARATOR,
            keccak256(abi_encode(PERMIT_TYPEHASH_VALUE, owner, spender, amount, nonce, deadline)),
        )
    )
    recovered: address = ecrecover(digest, convert(v, uint256), convert(r, uint256), convert(s, uint256))
    assert recovered != empty(address) and recovered == owner, "UniswapV2: INVALID_SIGNATURE"
    self.allowance[owner][spender] = amount
    log Approval(owner=owner, spender=spender, amount=amount)

@external
def transfer(receiver: address, amount: uint256) -> bool:
    self._transfer(msg.sender, receiver, amount)
    return True

@external
def transferFrom(owner: address, receiver: address, amount: uint256) -> bool:
    allowed: uint256 = self.allowance[owner][msg.sender]
    if allowed != max_value(uint256):
        assert allowed >= amount, "UniswapV2: INSUFFICIENT_ALLOWANCE"
        self.allowance[owner][msg.sender] = allowed - amount
    self._transfer(owner, receiver, amount)
    return True

@external
@view
def getReserves() -> (uint112, uint112, uint32):
    return self.reserve0, self.reserve1, self.blockTimestampLast

@external
def mint(receiver: address) -> uint256:
    self._lock()
    old_reserve0: uint112 = self.reserve0
    old_reserve1: uint112 = self.reserve1
    balance0_: uint256 = self._balance(self.token0)
    balance1_: uint256 = self._balance(self.token1)
    amount0: uint256 = balance0_ - convert(old_reserve0, uint256)
    amount1: uint256 = balance1_ - convert(old_reserve1, uint256)
    fee_on: bool = self._mint_fee(old_reserve0, old_reserve1)
    supply: uint256 = self.totalSupply
    liquidity: uint256 = 0
    if supply == 0:
        liquidity = self._sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
        self._mint(empty(address), MINIMUM_LIQUIDITY)
    else:
        liquidity = self._min(
            amount0 * supply // convert(old_reserve0, uint256),
            amount1 * supply // convert(old_reserve1, uint256),
        )
    assert liquidity > 0, "UniswapV2: INSUFFICIENT_LIQUIDITY_MINTED"
    self._mint(receiver, liquidity)
    self._update(balance0_, balance1_, old_reserve0, old_reserve1)
    if fee_on:
        self.kLast = balance0_ * balance1_
    log Mint(sender=msg.sender, amount0=amount0, amount1=amount1)
    self._unlock()
    return liquidity

@external
def burn(receiver: address) -> (uint256, uint256):
    self._lock()
    old_reserve0: uint112 = self.reserve0
    old_reserve1: uint112 = self.reserve1
    token0_: address = self.token0
    token1_: address = self.token1
    balance0_: uint256 = self._balance(token0_)
    balance1_: uint256 = self._balance(token1_)
    liquidity: uint256 = self.balanceOf[self]
    fee_on: bool = self._mint_fee(old_reserve0, old_reserve1)
    supply: uint256 = self.totalSupply
    amount0: uint256 = liquidity * balance0_ // supply
    amount1: uint256 = liquidity * balance1_ // supply
    assert amount0 > 0 and amount1 > 0, "UniswapV2: INSUFFICIENT_LIQUIDITY_BURNED"
    self._burn(self, liquidity)
    self._safe_transfer(token0_, receiver, amount0)
    self._safe_transfer(token1_, receiver, amount1)
    balance0_ = self._balance(token0_)
    balance1_ = self._balance(token1_)
    self._update(balance0_, balance1_, old_reserve0, old_reserve1)
    if fee_on:
        self.kLast = balance0_ * balance1_
    log Burn(sender=msg.sender, amount0=amount0, amount1=amount1, receiver=receiver)
    self._unlock()
    return amount0, amount1

@external
def swap(amount0Out: uint256, amount1Out: uint256, receiver: address, data: Bytes[65536]):
    self._lock()
    assert amount0Out > 0 or amount1Out > 0, "UniswapV2: INSUFFICIENT_OUTPUT_AMOUNT"
    old_reserve0: uint112 = self.reserve0
    old_reserve1: uint112 = self.reserve1
    assert amount0Out < convert(old_reserve0, uint256) and amount1Out < convert(old_reserve1, uint256), "UniswapV2: INSUFFICIENT_LIQUIDITY"
    token0_: address = self.token0
    token1_: address = self.token1
    assert receiver != token0_ and receiver != token1_, "UniswapV2: INVALID_TO"
    if amount0Out > 0:
        self._safe_transfer(token0_, receiver, amount0Out)
    if amount1Out > 0:
        self._safe_transfer(token1_, receiver, amount1Out)
    if len(data) > 0:
        extcall UniswapV2Callee(receiver).uniswapV2Call(msg.sender, amount0Out, amount1Out, data)

    balance0_: uint256 = self._balance(token0_)
    balance1_: uint256 = self._balance(token1_)
    amount0In: uint256 = 0
    amount1In: uint256 = 0
    reserve0_less_out: uint256 = convert(old_reserve0, uint256) - amount0Out
    reserve1_less_out: uint256 = convert(old_reserve1, uint256) - amount1Out
    if balance0_ > reserve0_less_out:
        amount0In = balance0_ - reserve0_less_out
    if balance1_ > reserve1_less_out:
        amount1In = balance1_ - reserve1_less_out
    assert amount0In > 0 or amount1In > 0, "UniswapV2: INSUFFICIENT_INPUT_AMOUNT"

    balance0_adjusted: uint256 = balance0_ * 1000 - amount0In * 3
    balance1_adjusted: uint256 = balance1_ * 1000 - amount1In * 3
    assert balance0_adjusted * balance1_adjusted >= convert(old_reserve0, uint256) * convert(old_reserve1, uint256) * 1000000, "UniswapV2: K"
    self._update(balance0_, balance1_, old_reserve0, old_reserve1)
    log Swap(sender=msg.sender, amount0In=amount0In, amount1In=amount1In, amount0Out=amount0Out, amount1Out=amount1Out, receiver=receiver)
    self._unlock()

@external
def skim(receiver: address):
    self._lock()
    token0_: address = self.token0
    token1_: address = self.token1
    self._safe_transfer(token0_, receiver, self._balance(token0_) - convert(self.reserve0, uint256))
    self._safe_transfer(token1_, receiver, self._balance(token1_) - convert(self.reserve1, uint256))
    self._unlock()

@external
def sync():
    self._lock()
    self._update(
        self._balance(self.token0),
        self._balance(self.token1),
        self.reserve0,
        self.reserve1,
    )
    self._unlock()

@internal
@view
def _balance(token: address) -> uint256:
    return staticcall ERC20(token).balanceOf(self)

@internal
def _safe_transfer(token: address, receiver: address, amount: uint256):
    assert extcall ERC20(token).transfer(receiver, amount, default_return_value=True), "UniswapV2: TRANSFER_FAILED"

@internal
def _lock():
    assert self.unlocked, "UniswapV2: LOCKED"
    self.unlocked = False

@internal
def _unlock():
    self.unlocked = True

@internal
def _transfer(owner: address, receiver: address, amount: uint256):
    assert self.balanceOf[owner] >= amount, "UniswapV2: INSUFFICIENT_BALANCE"
    self.balanceOf[owner] -= amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=owner, receiver=receiver, amount=amount)

@internal
def _mint(receiver: address, amount: uint256):
    self.totalSupply += amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=empty(address), receiver=receiver, amount=amount)

@internal
def _burn(owner: address, amount: uint256):
    assert self.balanceOf[owner] >= amount, "UniswapV2: INSUFFICIENT_BALANCE"
    self.balanceOf[owner] -= amount
    self.totalSupply -= amount
    log Transfer(sender=owner, receiver=empty(address), amount=amount)

@internal
def _update(balance0_: uint256, balance1_: uint256, old_reserve0: uint112, old_reserve1: uint112):
    assert balance0_ <= convert(max_value(uint112), uint256) and balance1_ <= convert(max_value(uint112), uint256), "UniswapV2: OVERFLOW"
    block_timestamp: uint32 = convert(block.timestamp % 2**32, uint32)
    last_timestamp: uint32 = self.blockTimestampLast
    time_elapsed: uint32 = 0
    if block_timestamp >= last_timestamp:
        time_elapsed = block_timestamp - last_timestamp
    else:
        time_elapsed = max_value(uint32) - last_timestamp + block_timestamp + 1
    if time_elapsed > 0 and old_reserve0 != 0 and old_reserve1 != 0:
        price0_increment: uint256 = unsafe_mul(
            convert(old_reserve1, uint256) * Q112 // convert(old_reserve0, uint256),
            convert(time_elapsed, uint256),
        )
        price1_increment: uint256 = unsafe_mul(
            convert(old_reserve0, uint256) * Q112 // convert(old_reserve1, uint256),
            convert(time_elapsed, uint256),
        )
        self.price0CumulativeLast = unsafe_add(self.price0CumulativeLast, price0_increment)
        self.price1CumulativeLast = unsafe_add(self.price1CumulativeLast, price1_increment)
    new_reserve0: uint112 = convert(balance0_, uint112)
    new_reserve1: uint112 = convert(balance1_, uint112)
    self.reserve0 = new_reserve0
    self.reserve1 = new_reserve1
    self.blockTimestampLast = block_timestamp
    log Sync(reserve0=new_reserve0, reserve1=new_reserve1)

@internal
def _mint_fee(old_reserve0: uint112, old_reserve1: uint112) -> bool:
    current_fee_to: address = staticcall Factory(self.factory).feeTo()
    fee_on: bool = current_fee_to != empty(address)
    last_k: uint256 = self.kLast
    if fee_on:
        if last_k != 0:
            root_k: uint256 = self._sqrt(convert(old_reserve0, uint256) * convert(old_reserve1, uint256))
            root_k_last: uint256 = self._sqrt(last_k)
            if root_k > root_k_last:
                liquidity: uint256 = self.totalSupply * (root_k - root_k_last) // (root_k * 5 + root_k_last)
                if liquidity > 0:
                    self._mint(current_fee_to, liquidity)
    elif last_k != 0:
        self.kLast = 0
    return fee_on

@internal
@pure
def _sqrt(y: uint256) -> uint256:
    z: uint256 = 0
    if y > 3:
        z = y
        x: uint256 = y // 2 + 1
        for _: uint256 in range(256):
            if x >= z:
                break
            z = x
            x = (y // x + x) // 2
    elif y != 0:
        z = 1
    return z

@internal
@pure
def _min(x: uint256, y: uint256) -> uint256:
    if x < y:
        return x
    return y
