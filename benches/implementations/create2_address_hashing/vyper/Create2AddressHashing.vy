# pragma version >=0.4.3,<0.5.0

saltValue: public(HashMap[bytes32, uint256])
ADDRESS_MODULUS: constant(uint256) = 1461501637330902918203684832716283019655932542976

event Recorded:
    salt: indexed(bytes32)
    predicted: address
    value: uint256

@external
@pure
def initCodeHash(amount: uint256) -> bytes32:
    return keccak256(convert(amount, bytes32))

@internal
@view
def _compute_address(salt: bytes32, amount: uint256) -> address:
    digest: bytes32 = keccak256(concat(0xff, convert(self, bytes20), salt, keccak256(convert(amount, bytes32))))
    return convert(convert(digest, uint256) % ADDRESS_MODULUS, address)

@external
@view
def computeAddress(salt: bytes32, amount: uint256) -> address:
    return self._compute_address(salt, amount)

@external
def record(salt: bytes32, amount: uint256) -> uint256:
    predicted: address = self._compute_address(salt, amount)
    self.saltValue[salt] = amount
    log Recorded(salt=salt, predicted=predicted, value=amount)
    return amount
