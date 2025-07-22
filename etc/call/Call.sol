// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// solc -o . --bin --bin-runtime Call.sol --overwrite --optimize --optimize-runs 10000

contract Cell {
    uint256 private value;

    constructor(uint256 value_) {
        value = value_;
    }

    function set(uint256 value_) external {
        value = value_;
    }

    function get() external view returns (uint256) {
        return value;
    }
}

contract Call {
    address private owner;
    address private target;

    constructor() {
        owner = msg.sender;
        Cell cell = new Cell(0x42); // CREATE
        target = address(cell);
    }

    function get_owner() external view returns (address) {
        return owner;
    }

    function set(uint256 value_) external {
        Cell(target).set(value_); // CALL
    }

    function get() external view returns (uint256) {
        return Cell(target).get(); // CALL
    }
}
