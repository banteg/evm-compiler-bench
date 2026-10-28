# pragma version >=0.4.3,<0.5.0

nonces: public(HashMap[address, uint256])

@internal
@pure
def _hash_permit(owner: address, spender: address, amount: uint256, nonce: uint256, deadline: uint256) -> bytes32:
    return keccak256(
        concat(
            convert(owner, bytes32),
            convert(spender, bytes32),
            convert(amount, bytes32),
            convert(nonce, bytes32),
            convert(deadline, bytes32),
        )
    )

@external
@pure
def hashPermit(owner: address, spender: address, amount: uint256, nonce: uint256, deadline: uint256) -> bytes32:
    return self._hash_permit(owner, spender, amount, nonce, deadline)

@external
def useNonce(owner: address) -> uint256:
    current: uint256 = self.nonces[owner]
    self.nonces[owner] = current + 1
    return current

@external
@view
def hashCurrentPermit(owner: address, spender: address, amount: uint256, deadline: uint256) -> bytes32:
    return self._hash_permit(owner, spender, amount, self.nonces[owner], deadline)
