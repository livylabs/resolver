// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title Tree
/// @notice Standalone Incremental Merkle Tree example for EVM experiments.
/// @dev This contract is intentionally not part of the attestation registry core.
contract Tree {
    uint8 public constant MAX_DEPTH = 32;

    uint8 public immutable DEPTH;
    uint256 public immutable MAX_LEAVES;

    uint256 public leafCount;
    bytes32 public root;

    mapping(uint256 level => bytes32 zeroHash) public zeros;
    mapping(uint256 level => bytes32 subtreeHash) public filledSubtrees;
    mapping(uint256 index => bytes32 leafHash) public leaves;

    error InvalidDepth();
    error TreeFull();
    error ZeroLeaf();
    error WrongProofDepth(uint256 expected, uint256 actual);

    event LeafInserted(uint256 indexed index, bytes32 indexed leaf, bytes32 root);

    constructor(uint8 treeDepth) {
        if (treeDepth == 0 || treeDepth > MAX_DEPTH) revert InvalidDepth();

        DEPTH = treeDepth;
        MAX_LEAVES = uint256(1) << treeDepth;

        bytes32 zeroHash;
        for (uint256 level; level < treeDepth; ++level) {
            zeros[level] = zeroHash;
            filledSubtrees[level] = zeroHash;
            zeroHash = hashPair(zeroHash, zeroHash);
        }

        root = zeroHash;
    }

    function insert(bytes32 leaf) external returns (uint256 index, bytes32 newRoot) {
        if (leaf == bytes32(0)) revert ZeroLeaf();

        index = leafCount;
        if (index == MAX_LEAVES) revert TreeFull();

        leafCount = index + 1;
        leaves[index] = leaf;

        bytes32 currentHash = leaf;
        uint256 path = index;

        for (uint256 level; level < DEPTH; ++level) {
            if (path & 1 == 0) {
                filledSubtrees[level] = currentHash;
                currentHash = hashPair(currentHash, zeros[level]);
            } else {
                currentHash = hashPair(filledSubtrees[level], currentHash);
            }

            path >>= 1;
        }

        root = currentHash;
        newRoot = currentHash;

        emit LeafInserted(index, leaf, newRoot);
    }

    function verifyCurrent(bytes32 leaf, uint256 index, bytes32[] calldata siblings) external view returns (bool) {
        if (siblings.length != DEPTH) revert WrongProofDepth(DEPTH, siblings.length);
        if (index >= leafCount) return false;

        return computeRoot(leaf, index, siblings) == root;
    }

    function computeRoot(bytes32 leaf, uint256 index, bytes32[] calldata siblings) public pure returns (bytes32) {
        bytes32 currentHash = leaf;
        uint256 path = index;

        for (uint256 level; level < siblings.length; ++level) {
            if (path & 1 == 0) {
                currentHash = hashPair(currentHash, siblings[level]);
            } else {
                currentHash = hashPair(siblings[level], currentHash);
            }

            path >>= 1;
        }

        return currentHash;
    }

    function hashPair(bytes32 left, bytes32 right) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(left, right));
    }
}
