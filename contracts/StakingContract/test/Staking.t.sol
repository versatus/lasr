// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/Staking.sol";
import "../src/Versa.sol";
import "../lib/openzeppelin-contracts/contracts/access/Ownable.sol";

contract StakingContractTest is Test {
    RollupStaking stakingContract;
    VersaToken vtoken;

    address deployer = address(1);
    address staker1 = address(2);
    address staker2 = address(3);
    address staker3 = address(4);
    address staker4 = address(5);
    address staker5 = address(6);

    function setUp() public {
        // Define the addresses of the stakers
        address[] memory stakers = new address[](5);
        stakers[0] = staker1;
        stakers[1] = staker2;
        stakers[2] = staker3;
        stakers[3] = staker4;
        stakers[4] = staker5;

        vtoken = new VersaToken();
        for (uint256 i = 0; i < stakers.length; i++) {
            vm.deal(stakers[i], 5 ether);
            vtoken.transfer(stakers[i], 1_000_000 * 1e18);
        }

        uint256 currentTime = block.timestamp;
        stakingContract = new RollupStaking(vtoken, 1e18, currentTime, 30 days);
        vtoken.transfer(address(stakingContract), 5_000_000 * 1e18);

        // Approve the StakingContract to spend stakers' tokens
        for (uint256 i = 0; i < stakers.length; i++) {
            vm.prank(stakers[i]);
            vtoken.approve(address(stakingContract), type(uint256).max);
        }
    }

    function testStakeAmount() public payable {
        uint256 stakeAmount = 0.1 ether; // Use the Ether sent as the stake amount

        vm.startPrank(staker1);
        stakingContract.stake{value: stakeAmount}();

        (uint256 amountStaked,,,) = stakingContract.stakers(staker1);
        assertEq(amountStaked, stakeAmount);

        // Stake additional amounts for testing
        stakingContract.stake{value: stakeAmount}();
        stakingContract.stake{value: stakeAmount}();
        stakingContract.stake{value: stakeAmount}();
        vm.stopPrank();
    }

    function testEarnRewards(uint256 _timeDelay) public {
        uint256 timeDelay = bound(_timeDelay, 2, stakingContract.unstakeCooldown() * 100);

        vm.startPrank(staker1);
        stakingContract.stake{value: 0.01 ether}();

        // Warp 1 week into the future
        vm.warp(block.timestamp + timeDelay);

        assertTrue(vtoken.balanceOf(staker1) == 1_000_000 * 1e18);

        // Claim rewards
        stakingContract.claimReward();

        // Check that user1's balance increased due to rewards

        assertTrue(vtoken.balanceOf(staker1) > 1_000_000 * 1e18);
        vm.stopPrank();
    }

    function testEarnedAndClaim() public {
        uint256 stakeAmount = 0.01 ether;

        vm.startPrank(staker1);
        stakingContract.stake{value: stakeAmount}();
        vm.stopPrank();

        assertEq(stakingContract.totalStakedAmount(), stakeAmount, "Total staked amount doesn't match.");

        vm.warp(block.timestamp + 7 days);

        // Calculate expected rewards
        uint256 expectedRewards = 1e18 * 7 days;

        // Verify the earned rewards for staker1
        uint256 actualEarned = stakingContract.calculateEarnedRewards(staker1);
        assertEq(actualEarned, expectedRewards, "Earned rewards don't match expected.");

        uint256 prevBalance = vtoken.balanceOf(staker1);
        vm.startPrank(staker1);
        stakingContract.claimReward();
        vm.stopPrank();
        uint256 updatedBalance = vtoken.balanceOf(staker1);
        assertEq(updatedBalance - prevBalance, expectedRewards, "Claimed rewards don't match expected rewards.");
    }

    function testNoRewardsAfterUnstaking() public {
        uint256 stakeAmount = 0.01 ether; 
        uint256 stakingDuration = 1 days; 
        uint256 unstakingDuration = 7 days; 

        vm.startPrank(staker1);
        stakingContract.stake{value: stakeAmount}();
        vm.stopPrank();


        // Simulate passing of time to the end of staking duration
        vm.warp(block.timestamp + stakingDuration);

        // Initiate unstake
        vm.startPrank(staker1);
        stakingContract.commenceUnstaking();
        vm.stopPrank();

        vm.startPrank(staker1);
        stakingContract.claimReward();
        vm.stopPrank();

        uint256 initialBalance = vtoken.balanceOf(staker1);

        // Simulate passing of time for unstaking 
        vm.warp(block.timestamp + unstakingDuration);

        vm.startPrank(staker1);
        vm.expectRevert("Insufficient rewards or funds");
        stakingContract.claimReward();
        vm.stopPrank();

        uint256 finalBalance = vtoken.balanceOf(staker1);
        assertEq(
            finalBalance - initialBalance,
            0,
            "Claimed Rewards should be zero"
        );
    }
}
