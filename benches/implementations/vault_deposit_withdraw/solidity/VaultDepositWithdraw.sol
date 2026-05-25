// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract VaultDepositWithdraw {
    mapping(address => uint256) public balanceOf;
    uint256 public totalShares;

    event Deposit(address indexed account, uint256 amount);
    event Withdraw(address indexed account, uint256 amount);

    function deposit() external payable returns (uint256) {
        require(msg.value > 0, "value");
        balanceOf[msg.sender] += msg.value;
        totalShares += msg.value;
        emit Deposit(msg.sender, msg.value);
        return msg.value;
    }

    function withdraw(uint256 amount) external returns (uint256) {
        require(balanceOf[msg.sender] >= amount, "shares");
        balanceOf[msg.sender] -= amount;
        totalShares -= amount;
        (bool ok,) = msg.sender.call{value: amount}("");
        require(ok, "eth");
        emit Withdraw(msg.sender, amount);
        return amount;
    }

    function totalAssets() external view returns (uint256) {
        return address(this).balance;
    }
}
