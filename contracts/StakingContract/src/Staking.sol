// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

// RollupStaking contract that allows users to stake tokens and earn rewards
contract RollupStaking is ReentrancyGuard, Ownable {
    // State variables to track staking information and rewards
    IERC20 public stakingToken;
    uint256 public totalStakedAmount;
    uint256 public totalRewardsAccrued;
    uint256 public rewardRatePerBlock;
    uint256 public lastUpdateTime;
    uint256 public rewardPerTokenAccrued;
    uint256 public unstakeCooldown = 7 days;
    uint256 public unstakeFeePercentage = 0;
    uint256 public launchTime;
    uint256 public closureTime;
    uint256 public unstakeFeeAccumulated;
    uint256 public rewardsPendingAccumulated;
    uint256 private constant FEE_CAP = 200;
    uint256 private constant FEE_PRECISION = 10000;
    uint256 private constant COOLDOWN_DURATION = 15 days;

    // Struct to store staker information
    struct Staker {
        uint256 stakedAmount;
        uint256 debtInRewards;
        uint256 earnedRewards;
        uint256 unstakeStartTime;
    }

    // Mapping to store staker information using their address as the key
    mapping(address => Staker) public stakers;

    // Events to log staking, rewards, and configuration updates
    event Staked(address indexed user, uint256 amount);
    event RewardPaid(address indexed user, uint256 reward);
    event UnstakeFeePercentageUpdated(uint256 newFeePercentage);
    event UnstakeCooldownUpdated(uint256 newCooldown);
    event UnstakeStarted(address indexed user);

    error ZeroStakeAmount();
    error StakingNotStartedYet();
    error StakingPeriodEnded();
    error AlreadyUnstaking();

    modifier updateReward(address account) {
        rewardPerTokenAccrued = rewardPerToken();
        rewardsPendingAccumulated = calculatePendingRewards();
        lastUpdateTime = lastApplicableTime();
        Staker storage staker = stakers[account];
        if (staker.unstakeStartTime == 0) {
            staker.earnedRewards = calculateEarnedRewards(account);
            staker.debtInRewards = rewardPerTokenAccrued;
        }
        _;
    }

    // Constructor to initialize the staking contract with parameters
    constructor(IERC20 _stakingToken, uint256 _rewardRatePerBlock, uint256 _launchTime, uint256 _duration)
        Ownable(msg.sender)
    {
        stakingToken = _stakingToken;
        rewardRatePerBlock = _rewardRatePerBlock;
        launchTime = _launchTime;
        closureTime = _launchTime + _duration;
    }

    function lastApplicableTime() public view returns (uint256) {
        if (block.timestamp < closureTime) {
            return block.timestamp;
        } else {
            return closureTime;
        }
    }

    // Function to calculate pending rewards for stakers
    function calculatePendingRewards() public view returns (uint256) {
        uint256 totalAccruedRewardsCached = totalRewardsAccrued;
        if (totalAccruedRewardsCached == 0) {
            return rewardsPendingAccumulated;
        }

        uint256 fullPendingRewards = calculateFullPendingRewards();
        uint256 remainderRewards = fullPendingRewards % totalAccruedRewardsCached;

        return rewardsPendingAccumulated + fullPendingRewards - remainderRewards;
    }

    // Function to calculate full pending rewards based on time difference and reward rate
    function calculateFullPendingRewards() private view returns (uint256) {
        return calculateTimeDifference() * rewardRatePerBlock * 1e18;
    }

    // Function to calculate time difference based on last update time and current time
    function calculateTimeDifference() private view returns (uint256) {
        return lastApplicableTime() - lastUpdateTime;
    }

    // Function to calculate reward per token based on total rewards accrued
    function rewardPerToken() public view returns (uint256) {
        if (totalRewardsAccrued == 0) {
            return rewardPerTokenAccrued;
        } else {
            uint256 timeSinceLastUpdate = lastApplicableTime() - lastUpdateTime;
            uint256 rewardPerTokenIncrease = timeSinceLastUpdate * rewardRatePerBlock * 1e18 / totalRewardsAccrued;
            return rewardPerTokenAccrued + rewardPerTokenIncrease;
        }
    }

    /**
     *  Calculates the total rewards earned by a staker based on their staked amount and reward rate.
     * If the staker has initiated an unstake, it returns the earned rewards directly.
     * Otherwise, it calculates the rewards based on the staked amount and reward rate per token.
     */
    function calculateEarnedRewards(address account) public view returns (uint256) {
        Staker storage staker = stakers[account];
        if (staker.unstakeStartTime != 0) {
            return staker.earnedRewards;
        }
        return (staker.stakedAmount * (rewardPerToken() - staker.debtInRewards) / 1e18) + staker.earnedRewards;
    }

    /*
     *  Allows a user to stake a specified amount of tokens into the staking contract.
     *  Updates the staker's information, total staked amount, total rewards accrued, and emits a Staked event.
     *
     * Requirements:
     * - The stake amount must be greater than 0.
     * - The current timestamp must be after the staking launch time.
     * - The current timestamp must be before the staking closure time.
     * - The staker must not have initiated an unstake process.
     *
     * @param amount The amount of tokens to stake.
     */
    function stake() external payable nonReentrant updateReward(msg.sender) {
        uint256 amount = msg.value;
        if (amount == 0) {
            revert ZeroStakeAmount();
        }
        if (block.timestamp < launchTime) {
            revert StakingNotStartedYet();
        }
        if (block.timestamp > closureTime) {
            revert StakingPeriodEnded();
        }
        Staker storage staker = stakers[msg.sender];
        require(staker.unstakeStartTime == 0, "Cannot stake after initiating unstake");
        totalStakedAmount += amount;
        totalRewardsAccrued += amount;
        staker.stakedAmount += amount;
        emit Staked(msg.sender, amount);
    }

    /**
     *  Allows a staker to claim their accumulated rewards.
     * This function first updates the staker's reward information (calling updateReward)
     * and then checks if the staker has any rewards and sufficient funds available in the contract
     * to cover the reward amount. If both conditions are met, the function:
     *  - Deducts the reward amount from the contract's pending rewards.
     *  - Resets the staker's earned rewards to zero.
     *  - Transfers the reward amount to the staker's address using the staking token.
     *  - Emits a `RewardPaid` event to record the successful claim.
     *
     * Requirements:
     * - The caller must be a valid staker.
     * - The staker must have earned rewards greater than zero.
     * - The contract must have sufficient funds (excluding pending rewards) to cover the reward amount.
     *
     * Emits a `RewardPaid` event upon successful claim.
     */
    function claimReward() external nonReentrant updateReward(msg.sender) {
        Staker storage staker = stakers[msg.sender];

        uint256 reward = staker.earnedRewards;
        if (reward == 0 || reward > getBalance()) {
            revert("Insufficient rewards or funds");
        }

        // Update balances atomically
        rewardsPendingAccumulated -= reward * 1e18;
        staker.earnedRewards = 0;

        // Transfer reward
        require(stakingToken.transfer(msg.sender, reward), "Reward transfer failed");

        emit RewardPaid(msg.sender, reward);
    }

    /**
     *  Allows the owner to withdraw any remaining tokens in the contract after the closure time.
     * This function can only be called by the contract owner and ensures there are sufficient
     * funds to cover pending rewards before withdrawing.
     *
     * Requirements:
     * - The caller must be the contract owner.
     * - The current block timestamp must be after the closure time.
     * - The free balance of the contract must be greater than the calculated pending rewards.
     *
     * Emits no event.
     */
    function withdrawRemainingTokens() external updateReward(address(0)) onlyOwner {
        uint256 freeBalance = getBalance();
        uint256 pendingRewards = calculatePendingRewards() / 1e18;
        if (block.timestamp <= closureTime || freeBalance <= pendingRewards) {
            revert("Withdrawal not allowed yet or insufficient funds");
        }

        // Calculate and transfer remaining balance atomically
        uint256 remainingBalance = freeBalance - pendingRewards;
        require(stakingToken.transfer(msg.sender, remainingBalance), "Token withdrawal failed");
    }

    /**
     *  Calculates the current free balance of the contract.
     * This function calculates the available balance by subtracting the total staked amount
     * and the accumulated unstake fees from the total balance of the staking token held by the contract.
     *
     *
     * Returns:
     * - The free balance of the contract (in ether).
     */
    function getBalance() internal view returns (uint256) {
        return stakingToken.balanceOf(address(this)) - totalStakedAmount - unstakeFeeAccumulated;
    }

    /**
     * Initiates the unstaking process for the caller.
     * This function updates the reward for the caller, checks if the staker has staked any ETH,
     * and ensures that the unstake process has not already been initiated.
     * It sets the unstake start time, deducts the staked amount from total rewards accrued,
     * and emits an UnstakeStarted event.
     * @return a boolean indicating whether the unstake process was successfully initiated.
     */
    function commenceUnstaking() external updateReward(msg.sender) returns (bool) {
        Staker storage staker = stakers[msg.sender];
        require(staker.stakedAmount > 0, "You haven't staked any ETH yet.");
        require(staker.unstakeStartTime == 0, "Unstake process already initiated.");

        staker.unstakeStartTime = block.timestamp;
        totalRewardsAccrued -= staker.stakedAmount;

        emit UnstakeStarted(msg.sender);
        return true;
    }

    /**
     * Sets the unstake fee percentage for unstaking.
     * This function allows the owner to set the unstake fee percentage, ensuring it does not exceed a predefined cap.
     * It updates the unstake fee percentage variable and emits an UnstakeFeePercentageUpdated event.
     * @param fee The new unstake fee percentage to be set.
     */
    function setUnstakeFeePercent(uint256 fee) external onlyOwner {
        require(fee <= FEE_CAP, "Unstake fee exceeds 5%, maximum allowed");
        unstakeFeePercentage = fee;
        emit UnstakeFeePercentageUpdated(fee);
    }

    /**
     * Sets the unstake cooldown time lock.
     * This function allows the owner to set the cooldown time lock for unstaking, ensuring it falls within a predefined range.
     * It updates the unstake cooldown variable and emits an UnstakeCooldownUpdated event.
     * @param coolDownTime The new cooldown time lock to be set, specified in days.
     */
    function setUnstakeTimeLock(uint256 coolDownTime) external onlyOwner {
        require(coolDownTime <= COOLDOWN_DURATION, "Cool Down time must be between 0 to 20 days");
        unstakeCooldown = coolDownTime;
        emit UnstakeCooldownUpdated(coolDownTime);
    }

     function withdrawStakingFees(uint256 amount) external onlyOwner {
        require(amount <= unstakeFeeAccumulated, "Amount exceeds unstakeFeeAccumulated");
        unstakeFeeAccumulated -= amount;
        require(payable(msg.sender).transfer(amount),"Fee withdrawal failed");
    }
}
