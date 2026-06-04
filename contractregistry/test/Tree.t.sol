// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Tree} from "../tree.sol";

contract TreeTest {
    function testInsertUpdatesRootAndLeafCount() public {
        Tree tree = new Tree(3);
        bytes32 leaf = keccak256("leaf-1");

        (uint256 index, bytes32 newRoot) = tree.insert(leaf);

        require(index == 0, "wrong insert index");
        require(tree.leafCount() == 1, "wrong leaf count");
        require(tree.leaves(index) == leaf, "leaf not stored");
        require(tree.root() == newRoot, "root not updated");
    }

    function testVerifyCurrentFirstLeaf() public {
        Tree tree = new Tree(3);
        bytes32 leaf = keccak256("leaf-1");

        tree.insert(leaf);

        bytes32[] memory siblings = new bytes32[](3);
        siblings[0] = tree.zeros(0);
        siblings[1] = tree.zeros(1);
        siblings[2] = tree.zeros(2);

        require(tree.verifyCurrent(leaf, 0, siblings), "valid proof rejected");
    }
}
