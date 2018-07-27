pragma solidity 0.4.24;

import "./SimpleLib.sol";

contract SimpleContract {
    using SimpleLib for uint;

    event ValueUpdated(uint indexed value);

    uint value;
    address owner;

    constructor(uint initial) public {
        value = initial;
        owner = msg.sender;
    }

    /*
     * Modifier that only permits the owner to access a function.
     */
    modifier ownerOnly() {
        require(msg.sender == owner);
        _;
    }

    function testAdd(uint a, uint b) public ownerOnly() {
        uint simpleUpdate = a.add(b);
        value = simpleUpdate;
    }

    function getValue() public view returns(uint) {
        return value;
    }

    function setValue(uint update) public payable ownerOnly() {
        value = update;
        emit ValueUpdated(update);
    }
}
