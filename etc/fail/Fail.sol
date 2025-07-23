// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// solc -o . --bin --bin-runtime Fail.sol --overwrite --optimize --optimize-runs 10000

contract Fail {
    address private owner;

    constructor() {
        owner = msg.sender;
    }

    function even_only(uint8 number) external pure returns (uint8) {
        require(number % 2 == 0, "even only");
        return number;
    }

    function is_owner() external view returns (bool) {
        require(msg.sender == owner, "not an owner");
        return true;
    }
}
