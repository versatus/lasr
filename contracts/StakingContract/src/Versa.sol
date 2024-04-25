pragma solidity ^0.8.13;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract VersaToken is ERC20 {
    constructor() ERC20("VersaToken", "Versa") {
        _mint(msg.sender, 100_000_000 * (10 ** uint256(decimals())));
    }
}
