pragma solidity 0.4.24;

contract SimpleLedger {
    mapping(address => uint) ledger;

    function add(address account) payable {
        ledger[account] += msg.value;
    }

    // used for testing
    function get(address account) returns(uint) {
        return ledger[account];
    }
}
