# pragma version >=0.4.3,<0.5.0

cloneValue: public(HashMap[bytes32, uint256])
ADDRESS_MODULUS: constant(uint256) = 1461501637330902918203684832716283019655932542976

event Cloned:
    salt: indexed(bytes32)
    predicted: address
    value: uint256

@external
@pure
def proxyCodeHash(implementation: address) -> bytes32:
    return keccak256(
        concat(
            0x3d602d80600a3d3981f3,
            0x363d3d373d3d3d363d73,
            convert(implementation, bytes20),
            0x5af43d82803e903d91602b57fd5bf3,
        )
    )

@internal
@view
def _predict_clone(implementation: address, salt: bytes32) -> address:
    code_hash: bytes32 = keccak256(
        concat(
            0x3d602d80600a3d3981f3,
            0x363d3d373d3d3d363d73,
            convert(implementation, bytes20),
            0x5af43d82803e903d91602b57fd5bf3,
        )
    )
    digest: bytes32 = keccak256(concat(0xff, convert(self, bytes20), salt, code_hash))
    return convert(convert(digest, uint256) % ADDRESS_MODULUS, address)

@external
@view
def predictClone(implementation: address, salt: bytes32) -> address:
    return self._predict_clone(implementation, salt)

@external
def recordClone(implementation: address, salt: bytes32, amount: uint256) -> uint256:
    predicted: address = self._predict_clone(implementation, salt)
    self.cloneValue[salt] = amount
    log Cloned(salt=salt, predicted=predicted, value=amount)
    return amount
