//! Employer key rotation while a payroll period is in progress.
//!
//! Covers the two-step admin rotation as it happens mid-period: the outgoing
//! admin keeps authority until the proposed admin accepts, a pending proposal
//! can be cancelled without side effects, and the company's employee roster
//! and commitments are untouched by the handover.

#![cfg(test)]

use payroll_registry::PayrollRegistryClient;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

fn setup() -> (Env, Address, u64, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(payroll_registry::PayrollRegistry, ());
    let client = PayrollRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let company_id = client.register_company(&admin, &treasury);

    (env, contract_id, company_id, admin, treasury)
}

#[test]
fn test_outgoing_admin_keeps_authority_while_rotation_is_pending() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);

    // Proposing does not move authority: the current admin is still recorded.
    assert_eq!(client.get_company(&company_id).admin, admin);
    let pending = client
        .get_pending_admin_rotation(&company_id)
        .expect("rotation should be pending");
    assert_eq!(pending.new_holder, new_admin);
    assert_eq!(pending.proposed_by, admin);
}

#[test]
fn test_accepted_mid_period_rotation_moves_authority_and_clears_pending() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &new_admin);

    assert_eq!(client.get_company(&company_id).admin, new_admin);
    assert!(client.get_pending_admin_rotation(&company_id).is_none());

    let version = client.get_admin_config_version(&company_id).unwrap();
    assert_eq!(version.version, 2);
    assert_eq!(version.updated_by, new_admin);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not the proposed admin")]
fn test_outgoing_admin_cannot_accept_its_own_rotation() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &admin);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not the company admin")]
fn test_outgoing_admin_cannot_propose_after_rotation() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);
    let third_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &new_admin);

    // The outgoing key has no authority once the handover is accepted.
    client.propose_admin_rotation(&company_id, &admin, &third_admin);
}

#[test]
fn test_new_admin_can_start_the_next_rotation() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);
    let third_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &new_admin);

    client.propose_admin_rotation(&company_id, &new_admin, &third_admin);

    let pending = client
        .get_pending_admin_rotation(&company_id)
        .expect("new admin's rotation should be pending");
    assert_eq!(pending.new_holder, third_admin);
    assert_eq!(pending.proposed_by, new_admin);
}

#[test]
fn test_cancelled_mid_period_rotation_keeps_admin_and_version() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    let version_before = client.get_admin_config_version(&company_id).unwrap();

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.cancel_admin_rotation(&company_id, &admin);

    assert_eq!(client.get_company(&company_id).admin, admin);
    assert!(client.get_pending_admin_rotation(&company_id).is_none());

    // A cancelled proposal is not a configuration change, so the revision holds.
    let version_after = client.get_admin_config_version(&company_id).unwrap();
    assert_eq!(version_after.version, version_before.version);
}

#[test]
fn test_employee_roster_survives_mid_period_rotation() {
    let (env, contract_id, company_id, admin, _treasury) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    let existing = Address::generate(&env);
    let existing_commitment = BytesN::from_array(&env, &[7u8; 32]);
    client.add_employee(&company_id, &existing, &existing_commitment);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &new_admin);

    // Employees and commitments recorded before the handover are unchanged.
    assert!(client.is_eligible(&company_id, &existing));
    assert_eq!(
        client.get_commitment(&company_id, &existing),
        existing_commitment
    );

    // The new admin can onboard employees for the rest of the period.
    let joiner = Address::generate(&env);
    let joiner_commitment = BytesN::from_array(&env, &[8u8; 32]);
    client.add_employee(&company_id, &joiner, &joiner_commitment);
    assert!(client.is_eligible(&company_id, &joiner));
}
