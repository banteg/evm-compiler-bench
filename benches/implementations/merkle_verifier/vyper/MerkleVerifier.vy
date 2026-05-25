# pragma version >=0.4.3,<0.5.0

@external
@pure
def verify(proof: DynArray[bytes32, 16], root: bytes32, leaf: bytes32) -> bool:
    computed: bytes32 = leaf
    for sibling: bytes32 in proof:
        if convert(computed, uint256) < convert(sibling, uint256):
            computed = keccak256(concat(computed, sibling))
        else:
            computed = keccak256(concat(sibling, computed))
    return computed == root

@external
@pure
def hashPair(left: bytes32, right: bytes32) -> bytes32:
    if convert(left, uint256) < convert(right, uint256):
        return keccak256(concat(left, right))
    return keccak256(concat(right, left))
