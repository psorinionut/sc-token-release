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
        let my_address: ManagedAddress = self.blockchain().get_caller();
        let activation_timestamp = self.blockchain().get_block_timestamp();
        require!(token_identifier.is_valid_esdt_identifier(), "Invalid token provided");
        self.token_identifier().set(&token_identifier);
        self.activation_timestamp().set(&activation_timestamp);
        self.set_owner(&my_address);
        self.setup_period_status().set(&true);
        Ok(())
    }

    // endpoints

    // Workflow
    // We first define all the groups. After that we whitelist an address (which also sets default starting balance = 0). 
    // After that we can assign to an address as many groups as we need
    #[endpoint]
    fn define_group(&self, group_identifier: BoxedBytes, 
        schedule_total_amount: BigUint,
        schedule_is_fixed_amount: bool,
        schedule_percent: u8,
        schedule_amount: BigUint,
        schedule_period: u64,
        schedule_ticks: u64,
    ) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.require_setup_period_live()?;

        let new_group = ScheduleType {
            schedule_total_amount,
            schedule_is_fixed_amount,
            schedule_percent,
            schedule_amount,
            schedule_period,
            schedule_ticks
        };

        self.setup_group(&group_identifier).set(&new_group);

        Ok(())
    }

    #[endpoint(removeGroup)]
    fn remove_group(&self, group_identifier: BoxedBytes) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.require_setup_period_live()?;
        require!(
            !self.setup_group(&group_identifier).is_empty(),
            "The group does not exist"
        );
 
        self.setup_group(&group_identifier).clear();
        Ok(())
    }

    #[endpoint]
    fn whitelist(&self, address: ManagedAddress, group_identifier: BoxedBytes) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.require_setup_period_live()?;
        require!(
            !self.setup_group(&group_identifier).is_empty(),
            "The group does not exist"
        );

        if self.whitelist_address(&address).is_empty() {
            let mut address_groups = Vec::new();
            address_groups.push(group_identifier);
            self.whitelist_address(&address).set(&address_groups);
        } else {
            let mut verifiy_address = self.whitelist_address(&address).get();
            if verifiy_address.iter().any(|i| i== &group_identifier) {
                //address already contains this group.
            } else {
                verifiy_address.push(group_identifier);
                self.whitelist_address(&address).set(&verifiy_address);
            };
        };

        self.claimed_balance(&address).set(&BigUint::zero());
        Ok(())
    }

    #[endpoint(removeWhitelist)]
    fn remove_whitelist(&self, address: ManagedAddress) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.require_setup_period_live()?;
        require!(
            !self.whitelist_address(&address).is_empty(),
            "The address is not whitelisted"
        );
 
        self.whitelist_address(&address).clear();
        self.claimed_balance(&address).clear();
        Ok(())
    }

    //To change a receiving address, the user registers a request, which is afterwards accepted or not by the owner
    #[endpoint(requestAddressChange)]
    fn request_address_change(&self, new_address: ManagedAddress) -> SCResult<()> {
        self.require_setup_period_ended()?;
        let user_address: ManagedAddress = self.blockchain().get_caller();
        self.change_address(&user_address).set(&new_address);
        Ok(())
    }

    #[endpoint(approveAddressChange)]
    fn approve_address_change(&self, user_address: ManagedAddress) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.require_setup_period_ended()?;
        require!(
            !self.change_address(&user_address).is_empty(),
            "The address is not whitelisted"
        );
        
        // Get old address values
        let new_address: ManagedAddress = self.change_address(&user_address).get();
        let user_groups: Vec<BoxedBytes> = self.whitelist_address(&user_address).get();
        let user_claimed_balance: BigUint = self.claimed_balance(&user_address).get();
         
        // Save the new address with the old address values
        self.whitelist_address(&new_address).set(&user_groups);
        self.claimed_balance(&new_address).set(&user_claimed_balance);

        // Delete the old address
        self.whitelist_address(&user_address).clear();
        self.claimed_balance(&user_address).clear();

        Ok(())
    }

    #[endpoint(endSetupPeriod)]
    fn end_setup_period(&self) -> SCResult<()> {
        only_owner!(self, "Permission denied");
        self.setup_period_status().set(&false);
        Ok(())
    }

    #[endpoint]
    fn claim_tokens(&self) -> SCResult<BigUint> {
        let token_identifier = self.token_identifier().get();
        self.require_setup_period_ended()?;
        let caller = self.blockchain().get_caller();
        let total_claimable_amount = self.calculate_claimable_tokens(&caller);
        let mut current_balance = self.claimed_balance(&caller).get();
        require!(&total_claimable_amount > &current_balance, "This address cannot currently claim any more tokens");
        let current_claimable_amount = total_claimable_amount - &current_balance;
        self.mint_and_send_tokens(&token_identifier, &caller, &current_claimable_amount);
        current_balance += &current_claimable_amount;
        self.claimed_balance(&caller).set(&current_balance);

        Ok(current_claimable_amount)
    }

    // views

    //Offers only the user the possibility to check the new requested address 
    #[view]
    fn verifiy_address_change(&self) -> ManagedAddress {
        let user_address: ManagedAddress = self.blockchain().get_caller();
        let new_address = self.change_address(&user_address).get();

        new_address
    }

    //Offers only the user the possibility to check the new requested address 
    #[view]
    fn verify_claimable_tokens(&self) -> BigUint {
        let caller = self.blockchain().get_caller();
        let total_claimable_amount = self.calculate_claimable_tokens(&caller);
        let current_balance = self.claimed_balance(&caller).get();

        if total_claimable_amount > current_balance{
            total_claimable_amount - current_balance
        } else {
            BigUint::zero()
        }
    }

    // private functions

    //Note that the SC must have the ESDTLocalMint or ESDTNftAddQuantity roles set, or this will fail with "action is not allowed".
    fn mint_and_send_tokens(&self, token_identifier: &TokenIdentifier, address: &ManagedAddress, amount: &BigUint) {
        self.send().esdt_local_mint(&token_identifier, 0, &amount);
        self.send().direct(&address, &token_identifier, 0, &amount, &[]);
    }

    fn calculate_claimable_tokens(&self, address: &ManagedAddress) -> (BigUint) {
        let starting_timestamp = self.activation_timestamp().get();
        let current_timestamp = self.blockchain().get_block_timestamp();
        let address_groups = self.whitelist_address(&address).get();

        let mut claimable_amount = BigUint::zero();

        for group_identifier in address_groups.iter()
        {
            let schedule_type = self.setup_group(&group_identifier).get();
            
            if schedule_type.schedule_ticks > 0 {
                let mut ticks = 1;
                while ticks <= schedule_type.schedule_ticks {
                    if current_timestamp - starting_timestamp >= schedule_type.schedule_period * ticks {
                        if schedule_type.schedule_is_fixed_amount{ //calculate fixed amount
                            claimable_amount += &schedule_type.schedule_amount;
                        } else { //calculate percentage
                            claimable_amount += &schedule_type.schedule_total_amount * (schedule_type.schedule_percent as u64) / (100 as u64);                           
                        }
                    }                   
                    ticks += 1;
                }
            }
        }

        claimable_amount
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

    // Can be used to test if the roles have been correctly set
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

    #[view(owner)]
    #[storage_get("owner")]
    fn get_owner(&self) -> ManagedAddress;

    #[storage_set("owner")]
    fn set_owner(&self, owner: &ManagedAddress);

    #[view(getActivationTimestamp)]
    #[storage_mapper("activationTimestamp")]
    fn activation_timestamp(&self) -> SingleValueMapper<u64>;
    
    #[view(getSetupPeriodStatus)]
    #[storage_mapper("setupPeriodStatus")]
    fn setup_period_status(&self) -> SingleValueMapper<bool>;

    #[view(getTokenIdentifier)]
    #[storage_mapper("tokenIdentifier")]
    fn token_identifier(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getAddressChanges)]
    #[storage_mapper("changeAddress")]
    fn change_address(&self, address: &ManagedAddress) -> SingleValueMapper<ManagedAddress>;

    #[view(getGroupSchedule)]
    #[storage_mapper("groupSchedule")]
    fn setup_group(&self, group_identifier: &BoxedBytes) -> SingleValueMapper<ScheduleType<Self::Api>>;
    
    #[storage_mapper("whitelistAddress")]
    fn whitelist_address(&self, address: &ManagedAddress) -> SingleValueMapper<Vec<BoxedBytes>>;

    #[view(getClaimedBalance)]
    #[storage_mapper("claimedBalance")]
    fn claimed_balance(&self, address: &ManagedAddress) -> SingleValueMapper<BigUint>;

}

    
