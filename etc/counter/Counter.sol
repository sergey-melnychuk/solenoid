// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// solc -o . --bin Counter.sol --overwrite

contract Counter {
    uint256 number;

    function inc() public {
        number++;
    }

    function dec() public {
        number--;
    }

    function set(uint256 value) public {
        number = value;
    }

    function get() public view returns (uint256) {
        return number;
    }
}
