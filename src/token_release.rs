#![no_std]

use crate::contract_data::ScheduleType;

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

mod contract_data;

#[elrond_wasm::contract]
pub trait TokenRelease {
    // The SC initializes with the setup period started. After the initial setup, the SC offers a function that ends the setup period. 
    // There is no function to start the setup period back on, so once the setup period is ended, it cannot be changed.
    #[init]
    fn init(&self, token_identifier: TokenIdentifier) -> SCResult<()> {
        let activation_timestamp = self.blockchain().get_block_timestamp();
        require!(token_identifier.is_valid_esdt_identifier(), "Invalid token provided");
        self.token_identifier().set(&token_identifier);
        self.activation_timestamp().set(&activation_timestamp);
        self.setup_period_status().set(&true);
        Ok(())
    }

    // endpoints

    // Workflow
    // First, all groups are defined. After that, an address can be assigned as many groups as needed
    #[only_owner]
    #[endpoint(addGroup)]
    fn add_group(&self, group_identifier: ManagedBuffer, 
        group_total_amount: BigUint,
        is_fixed_amount: bool,
        group_unlock_percent: u8,
        period_unlock_amount: BigUint,
        release_period: u64,
        release_ticks: u64,
    ) -> SCResult<()> {
        self.require_setup_period_live()?;
        require!(
            self.group_schedule(&group_identifier).is_empty(),
            "The group already exists"
        );
        require!(
            release_ticks > (0 as u64),
            "The schedule must have at least 1 unlock period"
        );
        require!(
            group_total_amount > BigUint::zero(),
            "The schedule must have a positive number of total tokens released"
        );
        if is_fixed_amount {
            require!(
                period_unlock_amount.clone() * BigUint::from(release_ticks) == group_total_amount,
                "The total number of tokens is invalid"
            );
        } else {
            require!(
                (group_unlock_percent as u64) * release_ticks == (100 as u64),
                "The final percentage is invalid"
            );
        }

        let new_group = ScheduleType {
            group_total_amount,
            is_fixed_amount,
            group_unlock_percent,
            period_unlock_amount,
            release_period,
            release_ticks
        };

        let mut token_supply = self.token_total_supply().get();
        token_supply += &new_group.group_total_amount;
        self.token_total_supply().set(&token_supply);
        self.group_schedule(&group_identifier).set(&new_group);

        Ok(())
    }

    #[only_owner]
    #[endpoint(removeGroup)]
    fn remove_group(&self, group_identifier: ManagedBuffer) -> SCResult<()> {
        self.require_setup_period_live()?;
        require!(
            !self.group_schedule(&group_identifier).is_empty(),
            "The group does not exist"
        );
 
        let group = self.group_schedule(&group_identifier).get();
        let mut token_supply = self.token_total_supply().get();
        token_supply -= &group.group_total_amount;
        self.token_total_supply().set(&token_supply);

        self.group_schedule(&group_identifier).clear();
        self.users_in_group(&group_identifier).clear();
        Ok(())
    }

    #[only_owner]
    #[endpoint(addUserGroup)]
    fn add_user_group(&self, address: ManagedAddress, group_identifier: ManagedBuffer) -> SCResult<()> {
        self.require_setup_period_live()?;
        require!(
            !self.group_schedule(&group_identifier).is_empty(),
            "The group does not exist"
        );

        if !self.user_groups(&address).is_empty() {
            let mut verify_address = self.user_groups(&address).get();
            if !verify_address.iter().any(|i| i == &group_identifier) {
                self.update_users_in_group(&group_identifier, true);
                verify_address.push(group_identifier);
                self.user_groups(&address).set(&verify_address);
            };
        } else {
            self.update_users_in_group(&group_identifier, true);
            let mut address_groups = Vec::new();
            address_groups.push(group_identifier);
            self.user_groups(&address).set(&address_groups);
        };

        Ok(())
    }

    #[only_owner]
    #[endpoint(removeUser)]
    fn remove_user(&self, address: ManagedAddress) -> SCResult<()> {
        self.require_setup_period_live()?;
        require!(
            !self.user_groups(&address).is_empty(),
            "The address is not defined"
        );
        let address_groups = self.user_groups(&address).get();
        for group_identifier in address_groups.iter()
        {
            self.update_users_in_group(&group_identifier, false);
        }
        self.user_groups(&address).clear();
        self.claimed_balance(&address).clear();
        Ok(())
    }

    //To change a receiving address, the user registers a request, which is afterwards accepted or not by the owner
    #[endpoint(requestAddressChange)]
    fn request_address_change(&self, new_address: ManagedAddress) -> SCResult<()> {
        self.require_setup_period_ended()?;
        let user_address = self.blockchain().get_caller();
        self.address_change_request(&user_address).set(&new_address);
        Ok(())
    }

    #[only_owner]
    #[endpoint(approveAddressChange)]
    fn approve_address_change(&self, user_address: ManagedAddress) -> SCResult<()> {
        self.require_setup_period_ended()?;
        require!(
            !self.address_change_request(&user_address).is_empty(),
            "The address does not have a change request"
        );
        
        // Get old address values
        let new_address = self.address_change_request(&user_address).get();
        let user_current_groups = self.user_groups(&user_address).get();
        let user_claimed_balance = self.claimed_balance(&user_address).get();
         
        // Save the new address with the old address values
        self.user_groups(&new_address).set(&user_current_groups);
        self.claimed_balance(&new_address).set(&user_claimed_balance);

        // Delete the old address
        self.user_groups(&user_address).clear();
        self.claimed_balance(&user_address).clear();

        // Delete the change request
        self.address_change_request(&user_address).clear();

        Ok(())
    }

    #[only_owner]
    #[endpoint(endSetupPeriod)]
    fn end_setup_period(&self) -> SCResult<()> {
        let token_identifier = self.token_identifier().get();
        self.require_local_burn_and_mint_roles_set(&token_identifier)?;
        self.setup_period_status().set(&false);
        let total_mint_tokens = self.token_total_supply().get();
        self.mint_all_tokens(&token_identifier, &total_mint_tokens);
        Ok(())
    }

    #[endpoint(claimTokens)]
    fn claim_tokens(&self) -> SCResult<BigUint> {
        self.require_setup_period_ended()?;
        let token_identifier = self.token_identifier().get();
        let caller = self.blockchain().get_caller();
        let total_claimable_amount = self.calculate_claimable_tokens(&caller);
        let mut current_balance = self.claimed_balance(&caller).get();
        require!(&total_claimable_amount > &current_balance, "This address cannot currently claim any more tokens");
        let current_claimable_amount = total_claimable_amount - &current_balance;
        self.send_tokens(&token_identifier, &caller, &current_claimable_amount);
        current_balance += &current_claimable_amount;
        self.claimed_balance(&caller).set(&current_balance);

        Ok(current_claimable_amount)
    }

    // views

    //Offers only the user the possibility to check the new requested address 
    #[view]
    fn verify_address_change(&self) -> ManagedAddress {
        let user_address = self.blockchain().get_caller();
        let new_address = self.address_change_request(&user_address).get();

        new_address
    }

    //Offers only the user the possibility to check how many tokens he can claim at the time of the request
    #[view]
    fn verify_claimable_tokens(&self) -> BigUint {
        let caller = self.blockchain().get_caller();
        let total_claimable_amount = self.calculate_claimable_tokens(&caller);
        let current_balance = self.claimed_balance(&caller).get();

        if total_claimable_amount > current_balance {
            total_claimable_amount - current_balance
        } else {
            BigUint::zero()
        }
    }

    // private functions

    fn send_tokens(&self, token_identifier: &TokenIdentifier, address: &ManagedAddress, amount: &BigUint) {
        self.send().direct(&address, &token_identifier, 0, &amount, &[]);
    }

    fn mint_all_tokens(&self, token_identifier: &TokenIdentifier, amount: &BigUint) {
        self.send().esdt_local_mint(&token_identifier, 0, &amount);
    }

    fn calculate_claimable_tokens(&self, address: &ManagedAddress) -> (BigUint) {
        let starting_timestamp = self.activation_timestamp().get();
        let current_timestamp = self.blockchain().get_block_timestamp();
        let address_groups = self.user_groups(&address).get();

        let mut claimable_amount = BigUint::zero();

        // Compute the total claimable amount at the time of the request, for all of the user groups
        for group_identifier in address_groups.iter()
        {
            let schedule_type = self.group_schedule(&group_identifier).get();
            let users_in_group_no = self.users_in_group(&group_identifier).get();
            
            let time_passed = current_timestamp - starting_timestamp;
            let mut periods_passed = time_passed / schedule_type.release_period;

            // Check if the user claims the tokens after (max periods no + n) passed, to compute the total amount based on the max number of periods
            // This means that the user cannot claim more than his total allocation and that the group total amount cannot be overpassed
            if periods_passed > schedule_type.release_ticks {
                periods_passed = schedule_type.release_ticks
            }

            if periods_passed > 0 {
                if schedule_type.is_fixed_amount {
                    claimable_amount +=  BigUint::from(periods_passed) * &schedule_type.period_unlock_amount / BigUint::from(users_in_group_no);
                } else {
                    claimable_amount +=  BigUint::from(periods_passed) * &schedule_type.group_total_amount * (schedule_type.group_unlock_percent as u64) / (100 as u64) / BigUint::from(users_in_group_no);                           
                }
            }
        }

        claimable_amount
    }

    fn update_users_in_group(&self, group_identifier: &ManagedBuffer, user_is_added: bool) {
        let mut users_in_group_no = self.users_in_group(&group_identifier).get();
        if user_is_added {
            users_in_group_no += &1;
        } else {
            users_in_group_no -= &1;
        }
        
        self.users_in_group(&group_identifier).set(&users_in_group_no);
    }

    fn require_setup_period_live(&self) -> SCResult<()> {
        require!(
            self.setup_period_status().get(),
            "Setup period has ended"
        );
        Ok(())
    }

    fn require_setup_period_ended(&self) -> SCResult<()> {
        require!(
            !(self.setup_period_status().get()),
            "Setup period is still active"
        );
        Ok(())
    }

    // Used to test if the roles have been correctly set
    fn require_local_burn_and_mint_roles_set(&self, token_identifier: &TokenIdentifier) -> SCResult<()> {
        let roles = self.blockchain().get_esdt_local_roles(token_identifier);
        require!(
            roles.contains(&EsdtLocalRole::Mint),
            "Local Mint role not set"
        );
        require!(
            roles.contains(&EsdtLocalRole::Burn),
            "Local Burn role not set"
        );
        Ok(())
    }


    // storage

    #[storage_mapper("activationTimestamp")]
    fn activation_timestamp(&self) -> SingleValueMapper<u64>;

    #[view(getTokenIdentifier)]
    #[storage_mapper("tokenIdentifier")]
    fn token_identifier(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getTokenTotalSupply)]
    #[storage_mapper("tokenTotalSupply")]
    fn token_total_supply(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("setupPeriodStatus")]
    fn setup_period_status(&self) -> SingleValueMapper<bool>;

    #[storage_mapper("addressChangeRequest")]
    fn address_change_request(&self, address: &ManagedAddress) -> SingleValueMapper<ManagedAddress>;

    #[storage_mapper("groupSchedule")]
    fn group_schedule(&self, group_identifier: &ManagedBuffer) -> SingleValueMapper<ScheduleType<Self::Api>>;  
    
    #[storage_mapper("userGroups")]
    fn user_groups(&self, address: &ManagedAddress) -> SingleValueMapper<Vec<ManagedBuffer>>;

    #[storage_mapper("usersInGroup")]
    fn users_in_group(&self, group_identifier: &ManagedBuffer) -> SingleValueMapper<u64>;

    #[storage_mapper("claimedBalance")]
    fn claimed_balance(&self, address: &ManagedAddress) -> SingleValueMapper<BigUint>;

}
