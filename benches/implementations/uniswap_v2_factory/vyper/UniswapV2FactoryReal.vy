# pragma version >=0.4.3,<0.5.0

interface Pair:
    def initialize(token0: address, token1: address): nonpayable

MAX_PAIR_CODE: constant(uint256) = 49152

feeTo: public(address)
feeToSetter: public(address)
getPair: public(HashMap[address, HashMap[address, address]])
pair_code: Bytes[MAX_PAIR_CODE]
all_pairs: HashMap[uint256, address]
all_pairs_length: uint256

event PairCreated:
    token0: indexed(address)
    token1: indexed(address)
    pair: address
    pair_count: uint256

@deploy
def __init__(pair_code_: Bytes[MAX_PAIR_CODE], fee_to_setter: address):
    self.pair_code = pair_code_
    self.feeToSetter = fee_to_setter

@external
@view
def allPairs(index: uint256) -> address:
    assert index < self.all_pairs_length
    return self.all_pairs[index]

@external
@view
def allPairsLength() -> uint256:
    return self.all_pairs_length

@external
def createPair(tokenA: address, tokenB: address) -> address:
    assert tokenA != tokenB, "UniswapV2: IDENTICAL_ADDRESSES"
    token0: address = tokenA
    token1: address = tokenB
    if convert(tokenB, uint256) < convert(tokenA, uint256):
        token0 = tokenB
        token1 = tokenA
    assert token0 != empty(address), "UniswapV2: ZERO_ADDRESS"
    assert self.getPair[token0][token1] == empty(address), "UniswapV2: PAIR_EXISTS"

    salt: bytes32 = keccak256(concat(convert(token0, bytes20), convert(token1, bytes20)))
    pair: address = raw_create(self.pair_code, salt=salt)
    extcall Pair(pair).initialize(token0, token1)
    self.getPair[token0][token1] = pair
    self.getPair[token1][token0] = pair
    self.all_pairs[self.all_pairs_length] = pair
    self.all_pairs_length += 1
    log PairCreated(token0=token0, token1=token1, pair=pair, pair_count=self.all_pairs_length)
    return pair

@external
def setFeeTo(fee_to: address):
    assert msg.sender == self.feeToSetter, "UniswapV2: FORBIDDEN"
    self.feeTo = fee_to

@external
def setFeeToSetter(fee_to_setter: address):
    assert msg.sender == self.feeToSetter, "UniswapV2: FORBIDDEN"
    self.feeToSetter = fee_to_setter
