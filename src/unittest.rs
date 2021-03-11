#[cfg(test)]
mod tests {
    use crate::contract::{handle, init, check_permission};
    use crate::expiration::Expiration;
    use crate::msg::{
        AccessLevel, Burn, ContractStatus, HandleAnswer, HandleMsg, InitConfig, InitMsg,
        QueryAnswer, Send, Transfer,
    };
    use crate::receiver::receive_nft_msg;
    use crate::state::{
        get_txs, json_load, json_may_load, load, may_load, AuthList, Config, Permission,
        PermissionType, TxAction, CONFIG_KEY, IDS_KEY, INDEX_KEY, MINTERS_KEY,
        PREFIX_ALL_PERMISSIONS, PREFIX_AUTHLIST, PREFIX_INFOS, PREFIX_OWNED, PREFIX_PRIV_META,
        PREFIX_PUB_META, PREFIX_RECEIVERS, PREFIX_VIEW_KEY, PREFIX_OWNER_PRIV, 
    };
    use crate::token::{Metadata, Token};
    use crate::viewing_key::{ViewingKey, VIEWING_KEY_SIZE};
    use cosmwasm_std::testing::*;
    use cosmwasm_std::{
        from_binary, to_binary, Api, Binary, BlockInfo, CanonicalAddr, Env, Extern, HandleResponse,
        HumanAddr, InitResponse, MessageInfo, QueryResponse, StdError, StdResult, WasmMsg,
    };
    use cosmwasm_storage::ReadonlyPrefixedStorage;

    use std::any::Any;
    use std::collections::{HashMap, HashSet};

    // Helper functions

    fn init_helper_default() -> (
        StdResult<InitResponse>,
        Extern<MockStorage, MockApi, MockQuerier>,
    ) {
        let mut deps = mock_dependencies(20, &[]);
        let env = mock_env("instantiator", &[]);

        let init_msg = InitMsg {
            name: "sec721".to_string(),
            symbol: "S721".to_string(),
            admin: Some(HumanAddr("admin".to_string())),
            entropy: "We're going to need a bigger boat".to_string(),
            config: None,
        };

        (init(&mut deps, env, init_msg), deps)
    }

    fn init_helper_with_config(
        public_token_supply: bool,
        public_owner: bool,
        enable_sealed_metadata: bool,
        unwrapped_metadata_is_private: bool,
        minter_may_update_metadata: bool,
        owner_may_update_metadata: bool,
        enable_burn: bool,
    ) -> (
        StdResult<InitResponse>,
        Extern<MockStorage, MockApi, MockQuerier>,
    ) {
        let mut deps = mock_dependencies(20, &[]);

        let env = mock_env("instantiator", &[]);
        let init_config: InitConfig = from_binary(&Binary::from(
            format!(
                "{{\"public_token_supply\":{},
            \"public_owner\":{},
            \"enable_sealed_metadata\":{},
            \"unwrapped_metadata_is_private\":{},
            \"minter_may_update_metadata\":{},
            \"owner_may_update_metadata\":{},
            \"enable_burn\":{}}}",
                public_token_supply,
                public_owner,
                enable_sealed_metadata,
                unwrapped_metadata_is_private,
                minter_may_update_metadata,
                owner_may_update_metadata,
                enable_burn
            )
            .as_bytes(),
        ))
        .unwrap();
        let init_msg = InitMsg {
            name: "sec721".to_string(),
            symbol: "S721".to_string(),
            admin: Some(HumanAddr("admin".to_string())),
            entropy: "We're going to need a bigger boat".to_string(),
            config: Some(init_config),
        };

        (init(&mut deps, env, init_msg), deps)
    }

    fn extract_error_msg<T: Any>(error: StdResult<T>) -> String {
        match error {
            Ok(_response) => panic!("Expected error, but had Ok response"),
            Err(err) => match err {
                StdError::GenericErr { msg, .. } => msg,
                _ => panic!(format!("Unexpected error result {:?}", err)),
            },
        }
    }
    /*
        fn extract_log(resp: StdResult<HandleResponse>) -> String {
            match resp {
                Ok(response) => response.log[0].value.clone(),
                Err(_err) => "These are not the logs you are looking for".to_string(),
            }
        }
    */

    // Init tests

    #[test]
    fn test_init_sanity() {
        // test default
        let (init_result, deps) = init_helper_default();
        assert_eq!(init_result.unwrap(), InitResponse::default());
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.status, ContractStatus::Normal.to_u8());
        assert_eq!(config.mint_cnt, 0);
        assert_eq!(config.tx_cnt, 0);
        assert_eq!(config.name, "sec721".to_string());
        assert_eq!(
            config.admin,
            deps.api
                .canonical_address(&HumanAddr("admin".to_string()))
                .unwrap()
        );
        assert_eq!(config.symbol, "S721".to_string());
        assert_eq!(config.token_supply_is_public, false);
        assert_eq!(config.owner_is_public, false);
        assert_eq!(config.sealed_metadata_is_enabled, false);
        assert_eq!(config.unwrap_to_private, false);
        assert_eq!(config.minter_may_update_metadata, true);
        assert_eq!(config.owner_may_update_metadata, false);
        assert_eq!(config.burn_is_enabled, false);

        // test config specification
        let (init_result, deps) =
            init_helper_with_config(true, true, true, true, false, true, false);
        assert_eq!(init_result.unwrap(), InitResponse::default());
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.status, ContractStatus::Normal.to_u8());
        assert_eq!(config.mint_cnt, 0);
        assert_eq!(config.tx_cnt, 0);
        assert_eq!(config.name, "sec721".to_string());
        assert_eq!(
            config.admin,
            deps.api
                .canonical_address(&HumanAddr("admin".to_string()))
                .unwrap()
        );
        assert_eq!(config.symbol, "S721".to_string());
        assert_eq!(config.token_supply_is_public, true);
        assert_eq!(config.owner_is_public, true);
        assert_eq!(config.sealed_metadata_is_enabled, true);
        assert_eq!(config.unwrap_to_private, true);
        assert_eq!(config.minter_may_update_metadata, false);
        assert_eq!(config.owner_may_update_metadata, true);
        assert_eq!(config.burn_is_enabled, false);
    }

    // Handle tests

    // test minting
    #[test]
    fn test_mint() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        // test minting when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test non-minter attempt
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Only designated minters are allowed to mint"));

        // sanity check
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: Some(Metadata {
                name: Some("MyNFTpriv".to_string()),
                description: Some("Nifty".to_string()),
                image: Some("privuri".to_string()),
            }),
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the token is in the id and index maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        let index = tokens.get("MyNFT").unwrap();
        let token_key = index.to_le_bytes();
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert_eq!(&*index_map.get(&index).unwrap(), "MyNFT");
        // verify all the token info
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &token_key).unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let admin_raw = deps
            .api
            .canonical_address(&HumanAddr("admin".to_string()))
            .unwrap();
        assert_eq!(token.owner, alice_raw);
        assert_eq!(token.permissions, Vec::new());
        assert!(token.unwrapped);
        // verify the token metadata
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &token_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, None);
        assert_eq!(pub_meta.image, Some("uri".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &token_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFTpriv".to_string()));
        assert_eq!(priv_meta.description, Some("Nifty".to_string()));
        assert_eq!(priv_meta.image, Some("privuri".to_string()));
        // verify token is in owner list
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let owned: HashSet<u32> = load(&owned_store, alice_raw.as_slice()).unwrap();
        assert!(owned.contains(&0));
        // verify mint tx was logged to both parties
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Mint {
                minter: HumanAddr("admin".to_string()),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(txs[0].memo, Some("Mint it baby!".to_string()));
        let tx2 = get_txs(&deps.api, &deps.storage, &admin_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // test minting with an existing token id
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: Some(Metadata {
                name: Some("MyNFTpriv".to_string()),
                description: Some("Nifty".to_string()),
                image: Some("privuri".to_string()),
            }),
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID is already in use"));

        // test minting without specifying recipient or id
        let handle_msg = HandleMsg::Mint {
            token_id: None,
            owner: None,
            public_metadata: Some(Metadata {
                name: Some("AdminNFT".to_string()),
                description: None,
                image: None,
            }),
            private_metadata: None,
            memo: Some("Admin wants his own".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify token is in the token list
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        let index = tokens.get("1").unwrap();
        let token_key = index.to_le_bytes();
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert_eq!(&*index_map.get(&index).unwrap(), "1");
        // verifyt token info
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &token_key).unwrap();
        let admin_raw = deps
            .api
            .canonical_address(&HumanAddr("admin".to_string()))
            .unwrap();
        assert_eq!(token.owner, admin_raw);
        assert_eq!(token.permissions, Vec::new());
        assert!(token.unwrapped);
        // verify metadata
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &token_key).unwrap();
        assert_eq!(pub_meta.name, Some("AdminNFT".to_string()));
        assert_eq!(pub_meta.description, None);
        assert_eq!(pub_meta.image, None);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &token_key).unwrap();
        assert!(priv_meta.is_none());
        // verify token is in the owner list
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let owned: HashSet<u32> = load(&owned_store, admin_raw.as_slice()).unwrap();
        assert!(owned.contains(&1));
        // verify mint tx was logged
        let txs = get_txs(&deps.api, &deps.storage, &admin_raw, 0, 10).unwrap();
        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].token_id, "1".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Mint {
                minter: HumanAddr("admin".to_string()),
                recipient: HumanAddr("admin".to_string()),
            }
        );
        assert_eq!(txs[0].memo, Some("Admin wants his own".to_string()));
        assert_eq!(txs[1].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[1].action,
            TxAction::Mint {
                minter: HumanAddr("admin".to_string()),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(txs[1].memo, Some("Mint it baby!".to_string()));
    }

    // test updating public metadata
    #[test]
    fn test_set_public_metadata() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "SNIP20".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: SNIP20 not found"));

        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "SNIP20".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata of token SNIP20"));

        // test setting metadata when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test not minter nor owner
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata"));

        // test owner tries but not allowed to change metadata
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata"));

        // test minter tries, but not allowed
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata"));

        // sanity check: minter updates
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetPublicMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &0u32.to_le_bytes()).unwrap();
        assert_eq!(pub_meta.name, Some("New Name".to_string()));
        assert_eq!(
            pub_meta.description,
            Some("I changed the metadata".to_string())
        );
        assert_eq!(pub_meta.image, Some("new uri".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &0u32.to_le_bytes()).unwrap();
        assert!(priv_meta.is_none());
    }

    #[test]
    fn test_set_private_metadata() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, true, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "SNIP20".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata of token SNIP20"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, true, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test trying to change sealed metadata before it has been unwrapped
        let (init_result, mut deps) =
            init_helper_with_config(true, false, true, true, true, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The private metadata of a sealed token can not be modified"));

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "SNIP20".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: SNIP20 not found"));

        // sanity check, minter changing metadata after owner unwrapped
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        let token_key = tokens.get("MyNFT").unwrap().to_le_bytes();
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &token_key).unwrap();
        assert!(token.unwrapped);
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("Minter changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &token_key).unwrap();
        assert_eq!(priv_meta.name, Some("New Name".to_string()));
        assert_eq!(
            priv_meta.description,
            Some("Minter changed the metadata".to_string())
        );
        assert_eq!(priv_meta.image, Some("new uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &token_key).unwrap();
        assert!(pub_meta.is_none());

        // test setting metadata when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test owner trying when not authorized
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("I changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Not authorized to update metadata of token MyNFT"));

        // test authorized owner creates new metadata when it didn't exist before
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, true, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: None,
                image: Some("uri".to_string()),
            }),
            private_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetPrivateMetadata {
            token_id: "MyNFT".to_string(),
            metadata: Metadata {
                name: Some("New Name".to_string()),
                description: Some("Owner changed the metadata".to_string()),
                image: Some("new uri".to_string()),
            },
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &token_key).unwrap();
        assert_eq!(priv_meta.name, Some("New Name".to_string()));
        assert_eq!(
            priv_meta.description,
            Some("Owner changed the metadata".to_string())
        );
        assert_eq!(priv_meta.image, Some("new uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &token_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, None);
        assert_eq!(pub_meta.image, Some("uri".to_string()));
    }

    // test Reveal
    #[test]
    fn test_reveal() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, true, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, true, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token MyNFT"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test reveal when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MySealedNFT".to_string()),
                description: Some("Sealed metadata test".to_string()),
                image: Some("sealed_uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test sealed metadata not enabled
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Sealed metadata functionality is not enabled for this contract"));

        // test someone other than owner tries to unwrap
        let (init_result, mut deps) =
            init_helper_with_config(false, false, true, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MySealedNFT".to_string()),
                description: Some("Sealed metadata test".to_string()),
                image: Some("sealed_uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token MyNFT"));

        // sanity check, unwrap to public metadata
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        let token_key = tokens.get("MyNFT").unwrap().to_le_bytes();
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &token_key).unwrap();
        assert!(priv_meta.is_none());
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &token_key).unwrap();
        assert_eq!(pub_meta.name, Some("MySealedNFT".to_string()));
        assert_eq!(
            pub_meta.description,
            Some("Sealed metadata test".to_string())
        );
        assert_eq!(pub_meta.image, Some("sealed_uri".to_string()));

        // test trying to unwrap token that has already been unwrapped
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("This token has already been unwrapped"));

        // sanity check, unwrap but keep private
        let (init_result, mut deps) =
            init_helper_with_config(false, false, true, true, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MySealedNFT".to_string()),
                description: Some("Sealed metadata test".to_string()),
                image: Some("sealed_uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Reveal {
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &token_key).unwrap();
        assert_eq!(priv_meta.name, Some("MySealedNFT".to_string()));
        assert_eq!(
            priv_meta.description,
            Some("Sealed metadata test".to_string())
        );
        assert_eq!(priv_meta.image, Some("sealed_uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &token_key).unwrap();
        assert!(pub_meta.is_none());
    }

    // test owner setting approval for specific addresses
    #[test]
    fn test_set_whitelisted_approval() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: NFT1 not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token NFT1"));

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Public 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My2".to_string()),
                description: Some("Public 2".to_string()),
                image: Some("URI 2".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg); // test burn when status prevents it
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My3".to_string()),
                description: Some("Public 3".to_string()),
                image: Some("URI 3".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My4".to_string()),
                description: Some("Public 4".to_string()),
                image: Some("URI 4".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test trying to set approval when status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));
        // setting approval is ok even during StopTransactions status
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // only allow the owner to use SetWhitelistedApproval
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token NFT1"));

        // try approving a token without specifying which token
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains(
            "Attempted to grant/revoke permission for a token, but did not specify a token ID"
        ));

        // try revoking a token approval without specifying which token
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::RevokeToken),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains(
            "Attempted to grant/revoke permission for a token, but did not specify a token ID"
        ));

        // sanity check
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let edmund_raw = deps
            .api
            .canonical_address(&HumanAddr("edmund".to_string()))
            .unwrap();
        let frank_raw = deps
            .api
            .canonical_address(&HumanAddr("frank".to_string()))
            .unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let nft1_key = 0u32.to_le_bytes();
        let nft2_key = 1u32.to_le_bytes();
        let nft3_key = 2u32.to_le_bytes();
        let nft4_key = 3u32.to_le_bytes();
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Public 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists has bob with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // verify it doesn't duplicate any entries if adding permissions that already
        // exist
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Public 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists has bob with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // verify changing an existing ALL expiration while adding token access
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtHeight(1000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm ALL permission with new expiration
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm NFT2 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft2_key).unwrap();
        assert_eq!(pub_meta.name, Some("My2".to_string()));
        assert_eq!(pub_meta.description, Some("Public 2".to_string()));
        assert_eq!(pub_meta.image, Some("URI 2".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft2_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists added bob's NFT2 transfer permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&1u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // verify default expiration of "never"
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm NFT3 permissions
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists added bob's nft3 transfer permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 3);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // verify revoking a token permission that never existed doesn't break anything
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::RevokeToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm NFT4 permissions
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm AuthLists are correct
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 3);
        assert!(!bob_auth.tokens[transfer_idx].contains(&3u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1500000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("edmund".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtHeight(2000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);

        // test revoking token permission
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::RevokeToken),
            // expiration is ignored when only performing revoking actions
            expires: Some(Expiration::AtTime(5)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm didn't affect ALL permissions
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 3);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1500000))
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        let edmund_oper_perm = all_perm.iter().find(|p| p.address == edmund_raw).unwrap();
        assert_eq!(
            edmund_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        assert_eq!(edmund_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(edmund_oper_perm.expirations[view_owner_idx], None);
        // confirm NFT2 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft2_key).unwrap();
        assert_eq!(pub_meta.name, Some("My2".to_string()));
        assert_eq!(pub_meta.description, Some("Public 2".to_string()));
        assert_eq!(pub_meta.image, Some("URI 2".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft2_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .is_none());
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists still has bob, but not with NFT2 transfer permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(!bob_auth.tokens[transfer_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());

        // test revoking a token permission when address has ALL permission removes the ALL
        // permission, and adds token permissions for all the other tokens not revoked
        // giving them the expiration of the removed ALL permission
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::RevokeToken),
            view_private_metadata: None,
            transfer: None,
            expires: Some(Expiration::AtTime(5)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm only bob's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        assert!(all_perm.iter().find(|p| p.address == bob_raw).is_none());
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1500000))
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        let edmund_oper_perm = all_perm.iter().find(|p| p.address == edmund_raw).unwrap();
        assert_eq!(
            edmund_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        assert_eq!(edmund_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(edmund_oper_perm.expirations[view_owner_idx], None);
        // confirm NFT1 permission added view_owner for bob with the old ALL permission
        // expiration, and did not touch the existing transfer permission for bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Public 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        // confirm NFT2 permission for bob and charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[transfer_idx], None);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        // confirm NFT3 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT4 permission for bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(1000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[transfer_idx], None);
        // confirm AuthLists still has bob, but not with NFT3 view_owner permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_owner_idx].len(), 3);
        assert!(bob_auth.tokens[view_owner_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_owner_idx].contains(&1u32));
        assert!(!bob_auth.tokens[view_owner_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_owner_idx].contains(&3u32));
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());

        // test revoking all view_owner permission
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            // will be ignored but specifying shouldn't screw anything up
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::None),
            view_private_metadata: None,
            transfer: None,
            // will be ignored but specifying shouldn't screw anything up
            expires: Some(Expiration::AtTime(5)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm only bob's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        assert!(all_perm.iter().find(|p| p.address == bob_raw).is_none());
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1500000))
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        let edmund_oper_perm = all_perm.iter().find(|p| p.address == edmund_raw).unwrap();
        assert_eq!(
            edmund_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        assert_eq!(edmund_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(edmund_oper_perm.expirations[view_owner_idx], None);
        // confirm NFT1 removed view_owner permission for bob, and did not touch the existing
        // transfer permission for bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Public 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        // confirm NFT2 permission removed bob but left and charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .is_none());
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        // confirm NFT3 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT4 permission removed bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm AuthLists still has bob, but only for NFT1 and 3 transfer permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());

        // test if approving a token for an address that already has ALL permission does
        // nothing if the given expiration is the same
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("edmund".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtHeight(2000)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm edmund still has ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        let edmund_oper_perm = all_perm.iter().find(|p| p.address == edmund_raw).unwrap();
        assert_eq!(
            edmund_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        assert_eq!(edmund_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(edmund_oper_perm.expirations[view_owner_idx], None);
        // confirm NFT4 permissions did not add edmund
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft4_key).unwrap();
        assert_eq!(pub_meta.name, Some("My4".to_string()));
        assert_eq!(pub_meta.description, Some("Public 4".to_string()));
        assert_eq!(pub_meta.image, Some("URI 4".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft4_key).unwrap();
        assert!(priv_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm edmund did not get added to AuthList
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        assert!(auth_list.iter().find(|a| a.address == edmund_raw).is_none());

        // test approving a token for an address that already has ALL permission updates that
        // token's permission's expiration, removes ALL permission, and sets token permission
        // for all other tokens using the ALL permission's expiration
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("edmund".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtHeight(3000)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm edmund's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == edmund_raw).is_none());
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1500000))
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        // confirm NFT1 added permission for edmund,
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT2 added permission for edmund,
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .is_none());
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT3 added permission for edmund and that the token data did not get modified
        // and did not touch the existing transfer permission for bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT4 permission added edmund with input expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        // confirm AuthLists added edmund for transferring on every tokens
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 3);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());

        // test that approving a token when the address has an expired ALL permission
        // deletes the ALL permission and performs like a regular ApproveToken (does not
        // add approve permission to all the other tokens)
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::Never),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 2000000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm davids's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let may_oper: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(may_oper.is_none());
        // confirm NFT3 did not add permission for david and that the token data did not get modified
        // and did not touch the existing transfer permission for bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT4 permission added david with input expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm AuthLists added david for transferring on NFT4
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 4);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&3u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());

        // giving frank ALL permission for later test
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("frank".to_string()),
            // will be ignored but specifying shouldn't screw anything up
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: None,
            expires: Some(Expiration::AtHeight(5000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm frank's ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let frank_oper_perm = all_perm.iter().find(|p| p.address == frank_raw).unwrap();
        assert_eq!(
            frank_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(5000))
        );
        assert_eq!(frank_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(frank_oper_perm.expirations[transfer_idx], None);
        // confirm NFT4 did not add permission for frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft4_key).unwrap();
        assert_eq!(pub_meta.name, Some("My4".to_string()));
        assert_eq!(pub_meta.description, Some("Public 4".to_string()));
        assert_eq!(pub_meta.image, Some("URI 4".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm AuthLists did not add frank
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 4);
        assert!(auth_list.iter().find(|a| a.address == frank_raw).is_none());
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&3u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());

        // test revoking a token permission when address has ALL permission removes the ALL
        // permission, and adds token permissions for all the other tokens not revoked
        // giving them the expiration of the removed ALL permission
        // This is same as above, but testing when the address has no AuthList already
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("frank".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::RevokeToken),
            view_private_metadata: None,
            transfer: None,
            // this will be ignored
            expires: Some(Expiration::AtTime(5)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm frank's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm NFT1 permission added view_owner for frank with the old ALL permission
        // expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 3);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        let frank_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .unwrap();
        assert_eq!(
            frank_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(5000))
        );
        assert_eq!(frank_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(frank_tok_perm.expirations[transfer_idx], None);
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Public 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        // confirm NFT2 permission
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 3);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        let frank_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .unwrap();
        assert_eq!(
            frank_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(5000))
        );
        assert_eq!(frank_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(frank_tok_perm.expirations[transfer_idx], None);
        // confirm NFT3 permissions do not include frank and that the token data did not get
        // modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT4 permission
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 3);
        let frank_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .unwrap();
        assert_eq!(
            frank_tok_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(5000))
        );
        assert_eq!(frank_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(frank_tok_perm.expirations[transfer_idx], None);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists added frank with view_owner permissions for all butNFT3
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 5);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&3u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());
        let frank_auth = auth_list.iter().find(|a| a.address == frank_raw).unwrap();
        assert_eq!(frank_auth.tokens[view_owner_idx].len(), 3);
        assert!(frank_auth.tokens[view_owner_idx].contains(&0u32));
        assert!(frank_auth.tokens[view_owner_idx].contains(&1u32));
        assert!(frank_auth.tokens[view_owner_idx].contains(&3u32));
        assert!(frank_auth.tokens[view_meta_idx].is_empty());
        assert!(frank_auth.tokens[transfer_idx].is_empty());

        // test granting ALL permission when the address has some token permissions
        // This should remove all the token permissions and the AuthList
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("frank".to_string()),
            token_id: None,
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: None,
            expires: Some(Expiration::AtHeight(2500)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm frank's ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let frank_oper_perm = all_perm.iter().find(|p| p.address == frank_raw).unwrap();
        assert_eq!(
            frank_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtHeight(2500))
        );
        assert_eq!(frank_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(frank_oper_perm.expirations[transfer_idx], None);
        // confirm NFT1 permission removed frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT2 permission removed frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        // confirm NFT4 permission removed frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists removed frank
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 4);
        assert!(auth_list.iter().find(|a| a.address == frank_raw).is_none());
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&3u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());

        // test revoking all permissions when address has ALL
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("frank".to_string()),
            token_id: None,
            view_owner: Some(AccessLevel::None),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm frank's ALL permission is gone
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm NFT1 permission removed frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm NFT2 permission removed frank
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        // confirm NFT4 permission removed frank
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == frank_raw)
            .is_none());
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(3000))
        );
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists removed frank
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 4);
        assert!(auth_list.iter().find(|a| a.address == frank_raw).is_none());
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 2);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&1u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let edmund_auth = auth_list.iter().find(|a| a.address == edmund_raw).unwrap();
        assert_eq!(edmund_auth.tokens[transfer_idx].len(), 4);
        assert!(edmund_auth.tokens[transfer_idx].contains(&0u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&1u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&2u32));
        assert!(edmund_auth.tokens[transfer_idx].contains(&3u32));
        assert!(edmund_auth.tokens[view_meta_idx].is_empty());
        assert!(edmund_auth.tokens[view_owner_idx].is_empty());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&3u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());

        // test revoking a token which is address' last permission
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::RevokeToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm NFT2 permission removed charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        assert!(token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .is_none());
        let edmund_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == edmund_raw)
            .unwrap();
        assert_eq!(edmund_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(edmund_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            edmund_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(2000))
        );
        // confirm AuthLists removed charlie
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert!(auth_list
            .iter()
            .find(|a| a.address == charlie_raw)
            .is_none());

        // verify that storage entry for AuthLists gets removed when all are gone
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::None),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::None),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("edmund".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::None),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // verify no ALL permissions left
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm NFT1 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm NFT2 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm NFT3 permissions are empty (and info is intact)
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm NFT4 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert!(token.permissions.is_empty());
        // verify no AuthLists left
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // verify revoking doesn't break anything when there are no permissions
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("edmund".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::RevokeToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::None),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // verify no ALL permissions left
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm NFT1 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm NFT2 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm NFT3 permissions are empty (and info is intact)
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft3_key).unwrap();
        assert_eq!(pub_meta.name, Some("My3".to_string()));
        assert_eq!(pub_meta.description, Some("Public 3".to_string()));
        assert_eq!(pub_meta.image, Some("URI 3".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft3_key).unwrap();
        assert!(priv_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm NFT4 permissions are empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft4_key).unwrap();
        assert!(token.permissions.is_empty());
        // verify no AuthLists left
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
    }

    // test approve from the cw721 spec
    #[test]
    fn test_cw721_approve() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, true, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("Not authorized to grant/revoke transfer permission for token MyNFT")
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test contract status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test unauthorized address attempt
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("Not authorized to grant/revoke transfer permission for token MyNFT")
        );

        // test expired operator attempt
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("MyNFT".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(500000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);

        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 2000000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Transfer authority for all tokens of alice has expired"));

        let tok_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;

        // test operator tries to grant permission to another operator.  This should
        // not do anything but end successfully
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm charlie still has ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        let charlie_oper_perm = all_perm.iter().find(|p| p.address == charlie_raw).unwrap();
        assert_eq!(
            charlie_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(500000))
        );
        assert_eq!(charlie_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_oper_perm.expirations[view_owner_idx], None);
        // confirm token permission did not add charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm charlie did not get added to Authlist
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check:  operator sets approval for an expired operator
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 750000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm charlie's expired ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == charlie_raw).is_none());
        // confirm token permission added charlie with default expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthList added charlie
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&0u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());

        // sanity check:  owner sets approval for an operator with only that one token
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(200)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm bob's ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm token permission added bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(200))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthList added bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // used to test auto-setting individual token permissions when only one token
        // of many is approved with a different expiration than an operator's expiration
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT2".to_string()),
                description: Some("metadata2".to_string()),
                image: Some("uri2".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("metadata3".to_string()),
                image: Some("uri3".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm david is an operator
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("david".to_string()),
            token_id: "MyNFT2".to_string(),
            expires: Some(Expiration::AtHeight(300)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm david's ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm MyNFT token permission added david with ALL permission's expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.permissions.len(), 3);
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm MyNFT2 token permission added david with input expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(300))
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok2_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT2".to_string()));
        assert_eq!(priv_meta.description, Some("metadata2".to_string()));
        assert_eq!(priv_meta.image, Some("uri2".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok2_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm MyNFT3 token permission added david with ALL permission's expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok3_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthList added david
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 3);
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 3);
        assert!(david_auth.tokens[transfer_idx].contains(&0u32));
        assert!(david_auth.tokens[transfer_idx].contains(&1u32));
        assert!(david_auth.tokens[transfer_idx].contains(&2u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());
    }

    // test Revoke from cw721 spec
    #[test]
    fn test_cw721_revoke() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("Not authorized to grant/revoke transfer permission for token MyNFT")
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test contract status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test unauthorized address attempt
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("Not authorized to grant/revoke transfer permission for token MyNFT")
        );

        // test expired operator attempt
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("MyNFT".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(500000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);

        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 2000000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Transfer authority for all tokens of alice has expired"));

        let tok_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;

        // test operator tries to revoke permission from another operator
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 1000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Can not revoke transfer permission from an existing operator"));

        // sanity check:  operator revokes approval from an expired operator will delete
        // the expired ALL permission
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 750000,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm charlie's expired ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == charlie_raw).is_none());
        // confirm token permission is still empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert!(token.permissions.is_empty());
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm AuthList is still empty
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check: operator approves, then revokes
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(200)),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm charlie does not have ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == charlie_raw).is_none());
        // confirm token permission added charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(200))
        );
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm charlie does not have ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == charlie_raw).is_none());
        // confirm token permission removed charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm AuthList removed charlie
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // verify revoking a non-existent permission does not break anything
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm charlie does not have ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        assert!(all_perm.iter().find(|p| p.address == charlie_raw).is_none());
        // confirm token does not list charlie
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm AuthList doesn not contain charlie
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check:  owner revokes token approval for an operator with only that one token
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm bob's ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm token permission is empty
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("metadata".to_string()));
        assert_eq!(priv_meta.image, Some("uri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        assert!(token.permissions.is_empty());
        // confirm AuthList does not contain bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // used to test auto-setting individual token permissions when only one token
        // of many is revoked from an operator
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT2".to_string()),
                description: Some("metadata2".to_string()),
                image: Some("uri2".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("metadata3".to_string()),
                image: Some("uri3".to_string()),
            }),
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::Never),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm david is an operator
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let david_oper_perm = all_perm.iter().find(|p| p.address == david_raw).unwrap();
        assert_eq!(
            david_oper_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(david_oper_perm.expirations[view_owner_idx], None);
        let handle_msg = HandleMsg::Revoke {
            spender: HumanAddr("david".to_string()),
            token_id: "MyNFT2".to_string(),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("alice".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm david's ALL permission was removed
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Option<Vec<Permission>> = json_may_load(&all_store, alice_key).unwrap();
        assert!(all_perm.is_none());
        // confirm MyNFT token permission added david with ALL permission's expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm MyNFT2 token permission does not contain david
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok2_key).unwrap();
        assert!(token.permissions.is_empty());
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok2_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT2".to_string()));
        assert_eq!(priv_meta.description, Some("metadata2".to_string()));
        assert_eq!(priv_meta.image, Some("uri2".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok2_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm MyNFT3 token permission added david with ALL permission's expiration
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok3_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let david_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == david_raw)
            .unwrap();
        assert_eq!(
            david_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        assert_eq!(david_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(david_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthList added david
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 2);
        assert!(david_auth.tokens[transfer_idx].contains(&0u32));
        assert!(!david_auth.tokens[transfer_idx].contains(&1u32));
        assert!(david_auth.tokens[transfer_idx].contains(&2u32));
        assert!(david_auth.tokens[view_meta_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());
    }

    // test burn
    #[test]
    fn test_burn() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test burn when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test when burn is disabled
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Burn functionality is not enabled for this token"));

        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, true);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is public
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, true);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is private
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("privmetadata".to_string()),
                image: Some("privuri".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("pubmetadata".to_string()),
                image: Some("puburi".to_string()),
            }),
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test unauthorized addres
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        // test expired token approval
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to token MyNFT has expired"));

        // test expired ALL approval
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to all tokens of alice has expired"));

        let tok_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();

        // sanity check: operator burns
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT".to_string(),
            memo: Some("Burn, baby, burn!".to_string()),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm token was removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.is_empty());
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.is_empty());
        // confirm token info was deleted from storage
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Option<Token> = json_may_load(&info_store, &tok_key).unwrap();
        assert!(token.is_none());
        // confirm the metadata has been deleted from storage
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &tok_key).unwrap();
        assert!(priv_meta.is_none());
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm the tx was logged to both parties
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Burn {
                owner: HumanAddr("alice".to_string()),
                burner: Some(HumanAddr("bob".to_string())),
            }
        );
        assert_eq!(txs[0].memo, Some("Burn, baby, burn!".to_string()));
        let tx2 = get_txs(&deps.api, &deps.storage, &bob_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm charlie's AuthList was removed because the only token was burned
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm the token was removed form the owner's list
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let owned: Option<HashSet<u32>> = may_load(&owned_store, alice_key).unwrap();
        assert!(owned.is_none());

        let transfer_idx = PermissionType::Transfer.to_u8() as usize;

        // sanity check: address with token permission burns it
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT2".to_string()),
                description: Some("privmetadata2".to_string()),
                image: Some("privuri2".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT2".to_string()),
                description: Some("pubmetadata2".to_string()),
                image: Some("puburi2".to_string()),
            }),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("privmetadata3".to_string()),
                image: Some("privuri3".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("pubmetadata3".to_string()),
                image: Some("puburi3".to_string()),
            }),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT2".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("david".to_string()),
            token_id: "MyNFT3".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT2".to_string(),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm token was removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(!tokens.contains_key("MyNFT2"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(!index_map.contains_key(&1u32));
        // confirm token info was deleted from storage
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Option<Token> = json_may_load(&info_store, &tok2_key).unwrap();
        assert!(token.is_none());
        // confirm MyNFT3 is intact
        let token: Token = json_load(&info_store, &tok3_key).unwrap();
        let david_perm = token.permissions.iter().find(|p| p.address == david_raw);
        assert!(david_perm.is_some());
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok3_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT3".to_string()));
        assert_eq!(priv_meta.description, Some("privmetadata3".to_string()));
        assert_eq!(priv_meta.image, Some("privuri3".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &tok3_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT3".to_string()));
        assert_eq!(pub_meta.description, Some("pubmetadata3".to_string()));
        assert_eq!(pub_meta.image, Some("puburi3".to_string()));
        // confirm the MyNFT2 metadata has been deleted from storage
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &tok2_key).unwrap();
        assert!(priv_meta.is_none());
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok2_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm the tx was logged to both parties
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs[0].token_id, "MyNFT2".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Burn {
                owner: HumanAddr("alice".to_string()),
                burner: Some(HumanAddr("charlie".to_string())),
            }
        );
        assert!(txs[0].memo.is_none());
        let tx2 = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm charlie's AuthList was removed because his only approved token was burned
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let charlie_auth = auth_list.iter().find(|a| a.address == charlie_raw);
        assert!(charlie_auth.is_none());
        let david_auth = auth_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[transfer_idx].len(), 1);
        assert!(david_auth.tokens[transfer_idx].contains(&2u32));
        // confirm the token was removed form the owner's list
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let owned: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert!(!owned.contains(&1u32));
        assert!(owned.contains(&2u32));

        // sanity check: owner burns
        let handle_msg = HandleMsg::BurnNft {
            token_id: "MyNFT3".to_string(),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm token was removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.is_empty());
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.is_empty());
        // confirm token info was deleted from storage
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Option<Token> = json_may_load(&info_store, &tok3_key).unwrap();
        assert!(token.is_none());
        // confirm the metadata has been deleted from storage
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &tok3_key).unwrap();
        assert!(priv_meta.is_none());
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok3_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm the tx was logged
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs[0].token_id, "MyNFT3".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Burn {
                owner: HumanAddr("alice".to_string()),
                burner: None,
            }
        );
        assert!(txs[0].memo.is_none());
        // confirm david's AuthList was removed because the only token was burned
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm the token was removed form the owner's list
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let owned: Option<HashSet<u32>> = may_load(&owned_store, alice_key).unwrap();
        assert!(owned.is_none());
    }

    // test batch burn
    #[test]
    fn test_batch_burn() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test burn when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::BurnNft {
            token_id: "NFT1".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test when burn is disabled
        let handle_msg = HandleMsg::BurnNft {
            token_id: "NFT1".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Burn functionality is not enabled for this token"));

        // set up for batch burn test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, true);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("privmetadata3".to_string()),
                image: Some("privuri3".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("pubmetadata3".to_string()),
                image: Some("puburi3".to_string()),
            }),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT7".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT8".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT7".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);

        // test bob burning a list, but trying to burn the same token twice
        let burns = vec![
            Burn {
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT3".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT6".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT6".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT8".to_string(),
                memo: Some("Phew!".to_string()),
            },
        ];
        let handle_msg = HandleMsg::BatchBurnNft {
            burns,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        // because the token no longer exists after burning it, it will say you are not
        // authorized if supply is private, and token not found if public
        assert!(error.contains("You are not authorized to perform this action on token NFT6"));

        // set up for batch burn test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, true);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("privmetadata3".to_string()),
                image: Some("privuri3".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("pubmetadata3".to_string()),
                image: Some("puburi3".to_string()),
            }),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT7".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT8".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT7".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);

        // test bob burning a list, but one is not authorized
        let burns = vec![
            Burn {
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT3".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT6".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT2".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT8".to_string(),
                memo: Some("Phew!".to_string()),
            },
        ];
        let handle_msg = HandleMsg::BatchBurnNft {
            burns,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token NFT2"));

        // set up for batch burn test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, true, false, false, false, true);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("privmetadata3".to_string()),
                image: Some("privuri3".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT3".to_string()),
                description: Some("pubmetadata3".to_string()),
                image: Some("puburi3".to_string()),
            }),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT7".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT8".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT7".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);

        // test bob burning NFT1 and 3 from alice with token permission,
        // burning NFT6 as the owner,
        // and burning NFT7 and NFT8 with ALL permission
        let burns = vec![
            Burn {
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT3".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT6".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT7".to_string(),
                memo: None,
            },
            Burn {
                token_id: "NFT8".to_string(),
                memo: Some("Phew!".to_string()),
            },
        ];
        let handle_msg = HandleMsg::BatchBurnNft {
            burns,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);

        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let tok1_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let tok6_key = 5u32.to_le_bytes();
        let tok7_key = 6u32.to_le_bytes();
        let tok8_key = 6u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let bob_key = bob_raw.as_slice();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let charlie_key = charlie_raw.as_slice();
        // confirm correct tokens were removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(!tokens.contains_key("NFT1"));
        assert!(tokens.contains_key("NFT2"));
        assert!(!tokens.contains_key("NFT3"));
        assert!(tokens.contains_key("NFT4"));
        assert!(tokens.contains_key("NFT5"));
        assert!(!tokens.contains_key("NFT6"));
        assert!(!tokens.contains_key("NFT7"));
        assert!(!tokens.contains_key("NFT8"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert_eq!(index_map.len(), 3);
        assert!(!index_map.contains_key(&0u32));
        assert!(index_map.contains_key(&1u32));
        assert!(!index_map.contains_key(&2u32));
        assert!(index_map.contains_key(&3u32));
        assert!(index_map.contains_key(&4u32));
        assert!(!index_map.contains_key(&5u32));
        assert!(!index_map.contains_key(&6u32));
        assert!(!index_map.contains_key(&7u32));
        // confirm token infos were deleted from storage
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Option<Token> = json_may_load(&info_store, &tok1_key).unwrap();
        assert!(token.is_none());
        let token: Option<Token> = json_may_load(&info_store, &tok3_key).unwrap();
        assert!(token.is_none());
        let token: Option<Token> = json_may_load(&info_store, &tok6_key).unwrap();
        assert!(token.is_none());
        let token: Option<Token> = json_may_load(&info_store, &tok7_key).unwrap();
        assert!(token.is_none());
        let token: Option<Token> = json_may_load(&info_store, &tok8_key).unwrap();
        assert!(token.is_none());
        // confirm NFT3 metadata has been deleted from storage
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &tok3_key).unwrap();
        assert!(priv_meta.is_none());
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Option<Metadata> = may_load(&pub_store, &tok3_key).unwrap();
        assert!(pub_meta.is_none());
        // confirm NFT2 is intact
        let token: Token = json_load(&info_store, &tok2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(
            charlie_tok_perm.expirations[view_meta_idx],
            Some(Expiration::Never)
        );
        assert_eq!(charlie_tok_perm.expirations[transfer_idx], None);
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(token.owner, alice_raw);
        assert!(!token.unwrapped);
        // confirm owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        // alice only owns NFT2
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 1);
        assert!(alice_owns.contains(&1u32));
        // bob owns NFT4 and NFT5
        let bob_owns: HashSet<u32> = load(&owned_store, bob_key).unwrap();
        assert_eq!(bob_owns.len(), 2);
        assert!(bob_owns.contains(&3u32));
        assert!(bob_owns.contains(&4u32));
        // charlie does not own any
        let charlie_owns: Option<HashSet<u32>> = may_load(&owned_store, charlie_key).unwrap();
        assert!(charlie_owns.is_none());
        // confirm AuthLists are correct
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        // alice gave charlie view metadata permission on NFT2
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 1);
        let charlie_auth = alice_list
            .iter()
            .find(|a| a.address == charlie_raw)
            .unwrap();
        assert_eq!(charlie_auth.tokens[view_meta_idx].len(), 1);
        assert!(charlie_auth.tokens[view_meta_idx].contains(&1u32));
        assert!(charlie_auth.tokens[transfer_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        // bob gave charlie view owner and view metadata permission on NFT5
        let bob_list: Vec<AuthList> = load(&auth_store, bob_key).unwrap();
        assert_eq!(bob_list.len(), 2);
        let charlie_auth = bob_list.iter().find(|a| a.address == charlie_raw).unwrap();
        assert_eq!(charlie_auth.tokens[view_meta_idx].len(), 1);
        assert!(charlie_auth.tokens[view_meta_idx].contains(&4u32));
        assert!(charlie_auth.tokens[transfer_idx].is_empty());
        assert_eq!(charlie_auth.tokens[view_owner_idx].len(), 1);
        assert!(charlie_auth.tokens[view_owner_idx].contains(&4u32));
        // bob gave alice view owner permission on NFT4 and NFT5
        // and transfer permission on NFT5
        let alice_auth = bob_list.iter().find(|a| a.address == alice_raw).unwrap();
        assert_eq!(alice_auth.tokens[transfer_idx].len(), 1);
        assert!(alice_auth.tokens[transfer_idx].contains(&4u32));
        assert!(alice_auth.tokens[view_meta_idx].is_empty());
        assert_eq!(alice_auth.tokens[view_owner_idx].len(), 2);
        assert!(alice_auth.tokens[view_owner_idx].contains(&3u32));
        assert!(alice_auth.tokens[view_owner_idx].contains(&4u32));
        // charlie has no tokens so should not have any AuthLists
        let charlie_list: Option<Vec<AuthList>> = may_load(&auth_store, charlie_key).unwrap();
        assert!(charlie_list.is_none());
        // confirm one of the txs
        let txs = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 1).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "NFT8".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Burn {
                owner: HumanAddr("charlie".to_string()),
                burner: Some(HumanAddr("bob".to_string())),
            }
        );
        assert_eq!(txs[0].memo, Some("Phew!".to_string()));
        let tx2 = get_txs(&deps.api, &deps.storage, &bob_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
    }

    // test transfer
    #[test]
    fn test_transfer() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test transfer when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is public
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is private
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("privmetadata".to_string()),
                image: Some("privuri".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("pubmetadata".to_string()),
                image: Some("puburi".to_string()),
            }),
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test unauthorized sender (but we'll give him view owner access)
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("MyNFT".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        // test expired token approval
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to token MyNFT has expired"));

        // test expired ALL approval
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to all tokens of alice has expired"));

        let tok_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let charlie_key = charlie_raw.as_slice();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let david_key = david_raw.as_slice();
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;

        // confirm that transfering to the same address that owns the token does not
        // erase the current permissions
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("alice".to_string()),
            token_id: "MyNFT".to_string(),
            memo: Some("Xfer it".to_string()),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token info is the same
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert_eq!(token.permissions.len(), 2);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(10))
        );
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[transfer_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert!(token.unwrapped);
        // confirm no transfer tx was logged (latest should be the mint tx)
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(
            txs[0].action,
            TxAction::Mint {
                minter: HumanAddr("admin".to_string()),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 1);
        assert!(alice_owns.contains(&0u32));
        // confirm charlie's and bob's AuthList were not changed
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 2);
        let charlie_auth = alice_list
            .iter()
            .find(|a| a.address == charlie_raw)
            .unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&0u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[view_owner_idx].len(), 1);
        assert!(bob_auth.tokens[view_owner_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[transfer_idx].is_empty());

        // sanity check: operator transfers
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("david".to_string()),
            token_id: "MyNFT".to_string(),
            memo: Some("Xfer it".to_string()),
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to david now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, david_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the metadata is intact
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("privmetadata".to_string()));
        assert_eq!(priv_meta.image, Some("privuri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &tok_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, Some("pubmetadata".to_string()));
        assert_eq!(pub_meta.image, Some("puburi".to_string()));
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("bob".to_string())),
                recipient: HumanAddr("david".to_string()),
            }
        );
        assert_eq!(txs[0].memo, Some("Xfer it".to_string()));
        let tx2 = get_txs(&deps.api, &deps.storage, &bob_raw, 0, 1).unwrap();
        let tx3 = get_txs(&deps.api, &deps.storage, &david_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        assert_eq!(tx2, tx3);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: Option<HashSet<u32>> = may_load(&owned_store, alice_key).unwrap();
        assert!(alice_owns.is_none());
        let david_owns: HashSet<u32> = load(&owned_store, david_key).unwrap();
        assert_eq!(david_owns.len(), 1);
        assert!(david_owns.contains(&0u32));
        // confirm charlie's and bob's AuthList were removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm david did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, david_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check: address with token permission xfers it to itself
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm token was not removed from the list
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to charlie now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, charlie_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the metadata is intact
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("privmetadata".to_string()));
        assert_eq!(priv_meta.image, Some("privuri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &tok_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, Some("pubmetadata".to_string()));
        assert_eq!(pub_meta.image, Some("puburi".to_string()));
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 10).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("david".to_string()),
                sender: Some(HumanAddr("charlie".to_string())),
                recipient: HumanAddr("charlie".to_string()),
            }
        );
        assert_eq!(txs[0].memo, None);
        let tx2 = get_txs(&deps.api, &deps.storage, &david_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let david_owns: Option<HashSet<u32>> = may_load(&owned_store, david_key).unwrap();
        assert!(david_owns.is_none());
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 1);
        assert!(charlie_owns.contains(&0u32));
        // confirm charlie's AuthList was removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, david_key).unwrap();
        assert!(auth_list.is_none());
        // confirm charlie did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, charlie_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check: owner xfers
        let handle_msg = HandleMsg::TransferNft {
            recipient: HumanAddr("alice".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to alice now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 1).unwrap();
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("charlie".to_string()),
                sender: None,
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(txs[0].memo, None);
        let tx2 = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let charlie_owns: Option<HashSet<u32>> = may_load(&owned_store, charlie_key).unwrap();
        assert!(charlie_owns.is_none());
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 1);
        assert!(alice_owns.contains(&0u32));
        // confirm charlie's AuthList was removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm charlie did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, charlie_key).unwrap();
        assert!(auth_list.is_none());
    }

    // test batch transfer
    #[test]
    fn test_batch_transfer() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test transfer when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let transfers = vec![Transfer {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
        }];
        let handle_msg = HandleMsg::BatchTransferNft {
            transfers,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let transfers = vec![Transfer {
            recipient: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            memo: None,
        }];

        // test token not found when supply is public
        let handle_msg = HandleMsg::BatchTransferNft {
            transfers,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let tok_key = 0u32.to_le_bytes();
        let tok5_key = 4u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let bob_key = bob_raw.as_slice();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let charlie_key = charlie_raw.as_slice();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let david_key = david_raw.as_slice();
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;

        // set up for batch transfer test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let transfers = vec![
            Transfer {
                recipient: HumanAddr("charlie".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("alice".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("bob".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("david".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
        ];

        // test transferring the same token among address the sender has ALL permission,
        // but then breaks when it gets to an address he does not have authority for
        let handle_msg = HandleMsg::BatchTransferNft {
            transfers,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token NFT1"));
        // confirm it didn't die until david tried to transfer itaway from bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, bob_raw);

        // set up for batch transfer test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let transfers = vec![
            Transfer {
                recipient: HumanAddr("charlie".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("alice".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("bob".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
        ];

        // test transferring the same token among address the sender has ALL permission
        // and verify the AuthLists are correct after all the transfers
        let handle_msg = HandleMsg::BatchTransferNft {
            transfers,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("NFT1"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token has the correct owner and the permissions were cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, bob_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm transfer txs were logged
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 10).unwrap();
        assert_eq!(txs.len(), 6);
        assert_eq!(
            txs[2].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("charlie".to_string()),
            }
        );
        assert_eq!(
            txs[1].action,
            TxAction::Transfer {
                from: HumanAddr("charlie".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("bob".to_string()),
            }
        );
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 2);
        assert!(!alice_owns.contains(&0u32));
        assert!(alice_owns.contains(&1u32));
        assert!(alice_owns.contains(&2u32));
        let bob_owns: HashSet<u32> = load(&owned_store, bob_key).unwrap();
        assert_eq!(bob_owns.len(), 3);
        assert!(bob_owns.contains(&0u32));
        assert!(bob_owns.contains(&3u32));
        assert!(bob_owns.contains(&4u32));
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 1);
        assert!(charlie_owns.contains(&5u32));
        // confirm authLists were updated correctly
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 2);
        let david_auth = alice_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[view_meta_idx].len(), 1);
        assert!(david_auth.tokens[view_meta_idx].contains(&2u32));
        assert!(david_auth.tokens[transfer_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[view_owner_idx].len(), 1);
        assert!(bob_auth.tokens[view_owner_idx].contains(&2u32));
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 2);
        assert!(bob_auth.tokens[view_meta_idx].contains(&1u32));
        assert!(bob_auth.tokens[view_meta_idx].contains(&2u32));
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        let bob_list: Vec<AuthList> = load(&auth_store, bob_key).unwrap();
        assert_eq!(bob_list.len(), 1);
        let alice_auth = bob_list.iter().find(|a| a.address == alice_raw).unwrap();
        assert_eq!(alice_auth.tokens[view_owner_idx].len(), 2);
        assert!(alice_auth.tokens[view_owner_idx].contains(&3u32));
        assert!(alice_auth.tokens[view_owner_idx].contains(&4u32));
        assert_eq!(alice_auth.tokens[view_meta_idx].len(), 1);
        assert!(alice_auth.tokens[view_meta_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[transfer_idx].len(), 1);
        assert!(alice_auth.tokens[transfer_idx].contains(&3u32));
        let charlie_list: Vec<AuthList> = load(&auth_store, charlie_key).unwrap();
        assert_eq!(charlie_list.len(), 1);
        let bob_auth = charlie_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&5u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());

        let transfers = vec![
            Transfer {
                recipient: HumanAddr("charlie".to_string()),
                token_id: "NFT1".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("alice".to_string()),
                token_id: "NFT5".to_string(),
                memo: None,
            },
            Transfer {
                recipient: HumanAddr("bob".to_string()),
                token_id: "NFT3".to_string(),
                memo: None,
            },
        ];

        // test bobs trnsfer two of his tokens and one of alice's
        let handle_msg = HandleMsg::BatchTransferNft {
            transfers,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        // confirm tokens have the correct owner and the permissions were cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, charlie_raw);
        assert!(token.permissions.is_empty());
        let token: Token = json_load(&info_store, &tok3_key).unwrap();
        assert_eq!(token.owner, bob_raw);
        assert!(token.permissions.is_empty());
        let token: Token = json_load(&info_store, &tok5_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.permissions.is_empty());
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 2);
        assert!(alice_owns.contains(&1u32));
        assert!(alice_owns.contains(&4u32));
        let bob_owns: HashSet<u32> = load(&owned_store, bob_key).unwrap();
        assert_eq!(bob_owns.len(), 2);
        assert!(bob_owns.contains(&2u32));
        assert!(bob_owns.contains(&3u32));
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 2);
        assert!(charlie_owns.contains(&0u32));
        assert!(charlie_owns.contains(&5u32));
        // confirm authLists were updated correctly
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 1);
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());
        let bob_list: Vec<AuthList> = load(&auth_store, bob_key).unwrap();
        assert_eq!(bob_list.len(), 1);
        let alice_auth = bob_list.iter().find(|a| a.address == alice_raw).unwrap();
        assert_eq!(alice_auth.tokens[view_owner_idx].len(), 1);
        assert!(alice_auth.tokens[view_owner_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[view_meta_idx].len(), 1);
        assert!(alice_auth.tokens[view_meta_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[transfer_idx].len(), 1);
        assert!(alice_auth.tokens[transfer_idx].contains(&3u32));
        let charlie_list: Vec<AuthList> = load(&auth_store, charlie_key).unwrap();
        assert_eq!(charlie_list.len(), 1);
        let bob_auth = charlie_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&5u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());
    }

    // test send
    #[test]
    fn test_send() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test send when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is public
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token not found when supply is private
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("privmetadata".to_string()),
                image: Some("privuri".to_string()),
            }),
            public_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("pubmetadata".to_string()),
                image: Some("puburi".to_string()),
            }),
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test unauthorized sender (but we'll give him view owner access)
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("MyNFT".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You are not authorized to perform this action on token MyNFT"));

        // test expired token approval
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to token MyNFT has expired"));

        // test expired ALL approval
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 100,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Access to all tokens of alice has expired"));

        let tok_key = 0u32.to_le_bytes();
        let tok2_key = 1u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let charlie_key = charlie_raw.as_slice();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let david_key = david_raw.as_slice();
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;

        // confirm that sending to the same address that owns the token does not
        // erase the current permissions
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "alice code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("alice".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: Some("Xfer it".to_string()),
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm that the ReceiveNft msg was created
        let alice_receive = receive_nft_msg(
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
            "MyNFT".to_string(),
            None,
            "alice code hash".to_string(),
            HumanAddr("alice".to_string()),
        )
        .unwrap();
        assert_eq!(handle_result.unwrap().messages[0], alice_receive);

        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token info is the same
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert_eq!(token.permissions.len(), 2);
        let charlie_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == charlie_raw)
            .unwrap();
        assert_eq!(charlie_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            charlie_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtHeight(10))
        );
        assert_eq!(charlie_tok_perm.expirations[view_owner_idx], None);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[transfer_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert!(token.unwrapped);
        // confirm no transfer tx was logged (latest should be the mint tx)
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(
            txs[0].action,
            TxAction::Mint {
                minter: HumanAddr("admin".to_string()),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 1);
        assert!(alice_owns.contains(&0u32));
        // confirm charlie's and bob's AuthList were not changed
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 2);
        let charlie_auth = alice_list
            .iter()
            .find(|a| a.address == charlie_raw)
            .unwrap();
        assert_eq!(charlie_auth.tokens[transfer_idx].len(), 1);
        assert!(charlie_auth.tokens[transfer_idx].contains(&0u32));
        assert!(charlie_auth.tokens[view_meta_idx].is_empty());
        assert!(charlie_auth.tokens[view_owner_idx].is_empty());
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[view_owner_idx].len(), 1);
        assert!(bob_auth.tokens[view_owner_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[transfer_idx].is_empty());

        // sanity check: operator sends
        // msg to go with ReceiveNft
        let send_msg = Some(
            to_binary(&HandleMsg::RevokeAll {
                operator: HumanAddr("zoe".to_string()),
                padding: None,
            })
            .unwrap(),
        );
        // register david's ReceiveNft
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "david code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("david".to_string()),
            token_id: "MyNFT".to_string(),
            msg: send_msg.clone(),
            memo: Some("Xfer it".to_string()),
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("bob".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm the receive nft msg was created
        let receive = receive_nft_msg(
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
            "MyNFT".to_string(),
            send_msg.clone(),
            "david code hash".to_string(),
            HumanAddr("david".to_string()),
        )
        .unwrap();
        assert_eq!(handle_result.unwrap().messages[0], receive);
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to david now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, david_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the metadata is intact
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("privmetadata".to_string()));
        assert_eq!(priv_meta.image, Some("privuri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &tok_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, Some("pubmetadata".to_string()));
        assert_eq!(pub_meta.image, Some("puburi".to_string()));
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("bob".to_string())),
                recipient: HumanAddr("david".to_string()),
            }
        );
        assert_eq!(txs[0].memo, Some("Xfer it".to_string()));
        let tx2 = get_txs(&deps.api, &deps.storage, &bob_raw, 0, 1).unwrap();
        let tx3 = get_txs(&deps.api, &deps.storage, &david_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        assert_eq!(tx2, tx3);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: Option<HashSet<u32>> = may_load(&owned_store, alice_key).unwrap();
        assert!(alice_owns.is_none());
        let david_owns: HashSet<u32> = load(&owned_store, david_key).unwrap();
        assert_eq!(david_owns.len(), 1);
        assert!(david_owns.contains(&0u32));
        // confirm charlie's and bob's AuthList were removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm david did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, david_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check: address with token permission xfers it to itself
        let handle_msg = HandleMsg::Approve {
            spender: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            expires: Some(Expiration::AtHeight(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        // register charlie's ReceiveNft
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "charlie code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("charlie".to_string()),
            token_id: "MyNFT".to_string(),
            msg: send_msg.clone(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(
            &mut deps,
            Env {
                block: BlockInfo {
                    height: 1,
                    time: 100,
                    chain_id: "cosmos-testnet-14002".to_string(),
                },
                message: MessageInfo {
                    sender: HumanAddr("charlie".to_string()),
                    sent_funds: vec![],
                },
                contract: cosmwasm_std::ContractInfo {
                    address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                },
                contract_key: Some("".to_string()),
                contract_code_hash: "".to_string(),
            },
            handle_msg,
        );
        // confirm the receive nft msg was created
        let receive = receive_nft_msg(
            HumanAddr("charlie".to_string()),
            HumanAddr("david".to_string()),
            "MyNFT".to_string(),
            send_msg.clone(),
            "charlie code hash".to_string(),
            HumanAddr("charlie".to_string()),
        )
        .unwrap();
        assert_eq!(handle_result.unwrap().messages[0], receive);
        // confirm token was not removed from the list
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to charlie now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, charlie_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the metadata is intact
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Metadata = load(&priv_store, &tok_key).unwrap();
        assert_eq!(priv_meta.name, Some("MyNFT".to_string()));
        assert_eq!(priv_meta.description, Some("privmetadata".to_string()));
        assert_eq!(priv_meta.image, Some("privuri".to_string()));
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &tok_key).unwrap();
        assert_eq!(pub_meta.name, Some("MyNFT".to_string()));
        assert_eq!(pub_meta.description, Some("pubmetadata".to_string()));
        assert_eq!(pub_meta.image, Some("puburi".to_string()));
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 10).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("david".to_string()),
                sender: Some(HumanAddr("charlie".to_string())),
                recipient: HumanAddr("charlie".to_string()),
            }
        );
        assert_eq!(txs[0].memo, None);
        let tx2 = get_txs(&deps.api, &deps.storage, &david_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let david_owns: Option<HashSet<u32>> = may_load(&owned_store, david_key).unwrap();
        assert!(david_owns.is_none());
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 1);
        assert!(charlie_owns.contains(&0u32));
        // confirm charlie's AuthList was removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, david_key).unwrap();
        assert!(auth_list.is_none());
        // confirm charlie did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, charlie_key).unwrap();
        assert!(auth_list.is_none());

        // sanity check: owner sends
        let handle_msg = HandleMsg::SendNft {
            contract: HumanAddr("alice".to_string()),
            token_id: "MyNFT".to_string(),
            msg: send_msg.clone(),
            memo: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        // confirm the receive nft msg was created
        let receive = receive_nft_msg(
            HumanAddr("charlie".to_string()),
            HumanAddr("charlie".to_string()),
            "MyNFT".to_string(),
            send_msg.clone(),
            "alice code hash".to_string(),
            HumanAddr("alice".to_string()),
        )
        .unwrap();
        assert_eq!(handle_result.unwrap().messages[0], receive);
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("MyNFT"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token belongs to alice now and permissions have been cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm the tx was logged to all involved parties
        let txs = get_txs(&deps.api, &deps.storage, &charlie_raw, 0, 1).unwrap();
        assert_eq!(txs[0].token_id, "MyNFT".to_string());
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("charlie".to_string()),
                sender: None,
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(txs[0].memo, None);
        let tx2 = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 1).unwrap();
        assert_eq!(txs, tx2);
        // confirm both owner lists are correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let charlie_owns: Option<HashSet<u32>> = may_load(&owned_store, charlie_key).unwrap();
        assert!(charlie_owns.is_none());
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 1);
        assert!(alice_owns.contains(&0u32));
        // confirm charlie's AuthList was removed because the only token was xferred
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
        // confirm charlie did not inherit any AuthLists from the xfer
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, charlie_key).unwrap();
        assert!(auth_list.is_none());
    }

    // test batch send
    #[test]
    fn test_batch_send() {
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("MyNFT".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: Some(Metadata {
                name: Some("MyNFT".to_string()),
                description: Some("metadata".to_string()),
                image: Some("uri".to_string()),
            }),
            public_metadata: None,
            memo: Some("Mint it baby!".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test send when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let sends = vec![Send {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
        }];
        let handle_msg = HandleMsg::BatchSendNft {
            sends,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::Normal,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let sends = vec![Send {
            contract: HumanAddr("bob".to_string()),
            token_id: "MyNFT".to_string(),
            msg: None,
            memo: None,
        }];

        // test token not found when supply is public
        let handle_msg = HandleMsg::BatchSendNft {
            sends,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: MyNFT not found"));

        let tok_key = 0u32.to_le_bytes();
        let tok5_key = 4u32.to_le_bytes();
        let tok3_key = 2u32.to_le_bytes();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let bob_key = bob_raw.as_slice();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let charlie_key = charlie_raw.as_slice();
        let david_raw = deps
            .api
            .canonical_address(&HumanAddr("david".to_string()))
            .unwrap();
        let david_key = david_raw.as_slice();
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;

        // set up for batch send test
        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT4".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT5".to_string()),
            owner: Some(HumanAddr("bob".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT6".to_string()),
            owner: Some(HumanAddr("charlie".to_string())),
            private_metadata: None,
            public_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT4".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("alice".to_string()),
            token_id: Some("NFT5".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT6".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("david".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "bob code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "charlie code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("charlie", &[]), handle_msg);
        // msg to go with ReceiveNft
        let send_msg = Some(
            to_binary(&HandleMsg::RevokeAll {
                operator: HumanAddr("zoe".to_string()),
                padding: None,
            })
            .unwrap(),
        );
        let sends = vec![
            Send {
                contract: HumanAddr("charlie".to_string()),
                token_id: "NFT1".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
            Send {
                contract: HumanAddr("alice".to_string()),
                token_id: "NFT1".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
            Send {
                contract: HumanAddr("bob".to_string()),
                token_id: "NFT1".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
        ];

        // test sending the same token among address the sender has ALL permission
        // and verify the AuthLists are correct after all the transfers
        let handle_msg = HandleMsg::BatchSendNft {
            sends,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("david", &[]), handle_msg);
        let resp = handle_result.unwrap();
        // confirm the receive nft msgs were created
        assert_eq!(resp.messages.len(), 2);
        let receive = receive_nft_msg(
            HumanAddr("david".to_string()),
            HumanAddr("alice".to_string()),
            "NFT1".to_string(),
            send_msg.clone(),
            "charlie code hash".to_string(),
            HumanAddr("charlie".to_string()),
        )
        .unwrap();
        assert_eq!(resp.messages[0], receive);
        let receive = receive_nft_msg(
            HumanAddr("david".to_string()),
            HumanAddr("alice".to_string()),
            "NFT1".to_string(),
            send_msg.clone(),
            "bob code hash".to_string(),
            HumanAddr("bob".to_string()),
        )
        .unwrap();
        assert_eq!(resp.messages[1], receive);
        // confirm token was not removed from the maps
        let tokens: HashMap<String, u32> = load(&deps.storage, IDS_KEY).unwrap();
        assert!(tokens.contains_key("NFT1"));
        let index_map: HashMap<u32, String> = load(&deps.storage, INDEX_KEY).unwrap();
        assert!(index_map.contains_key(&0u32));
        // confirm token has the correct owner and the permissions were cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, bob_raw);
        assert!(token.permissions.is_empty());
        assert!(token.unwrapped);
        // confirm transfer txs were logged
        let txs = get_txs(&deps.api, &deps.storage, &alice_raw, 0, 10).unwrap();
        assert_eq!(txs.len(), 6);
        assert_eq!(
            txs[2].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("charlie".to_string()),
            }
        );
        assert_eq!(
            txs[1].action,
            TxAction::Transfer {
                from: HumanAddr("charlie".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("alice".to_string()),
            }
        );
        assert_eq!(
            txs[0].action,
            TxAction::Transfer {
                from: HumanAddr("alice".to_string()),
                sender: Some(HumanAddr("david".to_string())),
                recipient: HumanAddr("bob".to_string()),
            }
        );
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 2);
        assert!(!alice_owns.contains(&0u32));
        assert!(alice_owns.contains(&1u32));
        assert!(alice_owns.contains(&2u32));
        let bob_owns: HashSet<u32> = load(&owned_store, bob_key).unwrap();
        assert_eq!(bob_owns.len(), 3);
        assert!(bob_owns.contains(&0u32));
        assert!(bob_owns.contains(&3u32));
        assert!(bob_owns.contains(&4u32));
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 1);
        assert!(charlie_owns.contains(&5u32));
        // confirm authLists were updated correctly
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 2);
        let david_auth = alice_list.iter().find(|a| a.address == david_raw).unwrap();
        assert_eq!(david_auth.tokens[view_meta_idx].len(), 1);
        assert!(david_auth.tokens[view_meta_idx].contains(&2u32));
        assert!(david_auth.tokens[transfer_idx].is_empty());
        assert!(david_auth.tokens[view_owner_idx].is_empty());
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[view_owner_idx].len(), 1);
        assert!(bob_auth.tokens[view_owner_idx].contains(&2u32));
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 2);
        assert!(bob_auth.tokens[view_meta_idx].contains(&1u32));
        assert!(bob_auth.tokens[view_meta_idx].contains(&2u32));
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        let bob_list: Vec<AuthList> = load(&auth_store, bob_key).unwrap();
        assert_eq!(bob_list.len(), 1);
        let alice_auth = bob_list.iter().find(|a| a.address == alice_raw).unwrap();
        assert_eq!(alice_auth.tokens[view_owner_idx].len(), 2);
        assert!(alice_auth.tokens[view_owner_idx].contains(&3u32));
        assert!(alice_auth.tokens[view_owner_idx].contains(&4u32));
        assert_eq!(alice_auth.tokens[view_meta_idx].len(), 1);
        assert!(alice_auth.tokens[view_meta_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[transfer_idx].len(), 1);
        assert!(alice_auth.tokens[transfer_idx].contains(&3u32));
        let charlie_list: Vec<AuthList> = load(&auth_store, charlie_key).unwrap();
        assert_eq!(charlie_list.len(), 1);
        let bob_auth = charlie_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&5u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());

        let sends = vec![
            Send {
                contract: HumanAddr("charlie".to_string()),
                token_id: "NFT1".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
            Send {
                contract: HumanAddr("alice".to_string()),
                token_id: "NFT5".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
            Send {
                contract: HumanAddr("bob".to_string()),
                token_id: "NFT3".to_string(),
                msg: send_msg.clone(),
                memo: None,
            },
        ];

        // test bobs trnsfer two of his tokens and one of alice's
        let handle_msg = HandleMsg::BatchSendNft {
            sends,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let resp = handle_result.unwrap();
        // confirm the receive nft msgs were created
        assert_eq!(resp.messages.len(), 2);
        let receive = receive_nft_msg(
            HumanAddr("bob".to_string()),
            HumanAddr("bob".to_string()),
            "NFT1".to_string(),
            send_msg.clone(),
            "charlie code hash".to_string(),
            HumanAddr("charlie".to_string()),
        )
        .unwrap();
        assert_eq!(resp.messages[0], receive);
        let receive = receive_nft_msg(
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
            "NFT3".to_string(),
            send_msg.clone(),
            "bob code hash".to_string(),
            HumanAddr("bob".to_string()),
        )
        .unwrap();
        assert_eq!(resp.messages[1], receive);
        // confirm tokens have the correct owner and the permissions were cleared
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &tok_key).unwrap();
        assert_eq!(token.owner, charlie_raw);
        assert!(token.permissions.is_empty());
        let token: Token = json_load(&info_store, &tok3_key).unwrap();
        assert_eq!(token.owner, bob_raw);
        assert!(token.permissions.is_empty());
        let token: Token = json_load(&info_store, &tok5_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.permissions.is_empty());
        // confirm the owner list is correct
        let owned_store = ReadonlyPrefixedStorage::new(PREFIX_OWNED, &deps.storage);
        let alice_owns: HashSet<u32> = load(&owned_store, alice_key).unwrap();
        assert_eq!(alice_owns.len(), 2);
        assert!(alice_owns.contains(&1u32));
        assert!(alice_owns.contains(&4u32));
        let bob_owns: HashSet<u32> = load(&owned_store, bob_key).unwrap();
        assert_eq!(bob_owns.len(), 2);
        assert!(bob_owns.contains(&2u32));
        assert!(bob_owns.contains(&3u32));
        let charlie_owns: HashSet<u32> = load(&owned_store, charlie_key).unwrap();
        assert_eq!(charlie_owns.len(), 2);
        assert!(charlie_owns.contains(&0u32));
        assert!(charlie_owns.contains(&5u32));
        // confirm authLists were updated correctly
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let alice_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(alice_list.len(), 1);
        let bob_auth = alice_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());
        let bob_list: Vec<AuthList> = load(&auth_store, bob_key).unwrap();
        assert_eq!(bob_list.len(), 1);
        let alice_auth = bob_list.iter().find(|a| a.address == alice_raw).unwrap();
        assert_eq!(alice_auth.tokens[view_owner_idx].len(), 1);
        assert!(alice_auth.tokens[view_owner_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[view_meta_idx].len(), 1);
        assert!(alice_auth.tokens[view_meta_idx].contains(&3u32));
        assert_eq!(alice_auth.tokens[transfer_idx].len(), 1);
        assert!(alice_auth.tokens[transfer_idx].contains(&3u32));
        let charlie_list: Vec<AuthList> = load(&auth_store, charlie_key).unwrap();
        assert_eq!(charlie_list.len(), 1);
        let bob_auth = charlie_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        assert_eq!(bob_auth.tokens[view_meta_idx].len(), 1);
        assert!(bob_auth.tokens[view_meta_idx].contains(&5u32));
        assert!(bob_auth.tokens[transfer_idx].is_empty());
    }

    // test register receive_nft
    #[test]
    fn test_register_receive_nft() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test register when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "alice code hash".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still register when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // sanity check
        let handle_msg = HandleMsg::RegisterReceiveNft {
            code_hash: "alice code hash".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let store = ReadonlyPrefixedStorage::new(PREFIX_RECEIVERS, &deps.storage);
        let hash: String = load(
            &store,
            deps.api
                .canonical_address(&HumanAddr("alice".to_string()))
                .unwrap()
                .as_slice(),
        )
        .unwrap();
        assert_eq!(&hash, "alice code hash");
    }

    // test create viewing key
    #[test]
    fn test_create_viewing_key() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test creating a key when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::CreateViewingKey {
            entropy: "blah".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still create a key when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::CreateViewingKey {
            entropy: "blah".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        assert!(
            handle_result.is_ok(),
            "handle() failed: {}",
            handle_result.err().unwrap()
        );
        let answer: HandleAnswer = from_binary(&handle_result.unwrap().data.unwrap()).unwrap();

        let key_str = match answer {
            HandleAnswer::ViewingKey { key } => key,
            _ => panic!("NOPE"),
        };
        let key = ViewingKey(key_str);
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let key_store = ReadonlyPrefixedStorage::new(PREFIX_VIEW_KEY, &deps.storage);
        let saved_vk: [u8; VIEWING_KEY_SIZE] = load(&key_store, alice_raw.as_slice()).unwrap();
        assert!(key.check_viewing_key(&saved_vk));
    }

    // test set viewing key
    #[test]
    fn test_set_viewing_key() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test setting a key when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetViewingKey {
            key: "blah".to_string(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still set a key when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let handle_msg = HandleMsg::SetViewingKey {
            key: "blah".to_string(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let key = ViewingKey("blah".to_string());
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let key_store = ReadonlyPrefixedStorage::new(PREFIX_VIEW_KEY, &deps.storage);
        let saved_vk: [u8; VIEWING_KEY_SIZE] = load(&key_store, alice_raw.as_slice()).unwrap();
        assert!(key.check_viewing_key(&saved_vk));
    }

    // test add minters
    #[test]
    fn test_add_minters() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test adding minters when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let minters = vec![
            HumanAddr("alice".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
        ];
        let handle_msg = HandleMsg::AddMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still add minters when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test non admin trying to add minters
        let handle_msg = HandleMsg::AddMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("This is an admin command and can only be run from the admin address")
        );

        // sanity check
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let admin_raw = deps
            .api
            .canonical_address(&HumanAddr("admin".to_string()))
            .unwrap();
        // verify the minters we will add are not already in the list
        assert!(!cur_minter.contains(&alice_raw));
        assert!(!cur_minter.contains(&bob_raw));
        let handle_msg = HandleMsg::AddMinters {
            minters,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the new minters were added
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 3);
        assert!(cur_minter.contains(&alice_raw));
        assert!(cur_minter.contains(&bob_raw));
        assert!(cur_minter.contains(&admin_raw));

        // let's try an empty list to see if it breaks
        let minters = vec![];
        let handle_msg = HandleMsg::AddMinters {
            minters,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify it's the same list
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 3);
        assert!(cur_minter.contains(&alice_raw));
        assert!(cur_minter.contains(&bob_raw));
        assert!(cur_minter.contains(&admin_raw));
    }

    // test remove minters
    #[test]
    fn test_remove_minters() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test removing minters when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let minters = vec![
            HumanAddr("alice".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("charlie".to_string()),
            HumanAddr("bob".to_string()),
        ];
        let handle_msg = HandleMsg::RemoveMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still remove minters when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test non admin trying to remove minters
        let handle_msg = HandleMsg::RemoveMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("This is an admin command and can only be run from the admin address")
        );

        // sanity check
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let admin_raw = deps
            .api
            .canonical_address(&HumanAddr("admin".to_string()))
            .unwrap();
        let handle_msg = HandleMsg::AddMinters {
            minters: minters.clone(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the new minters were added
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 4);
        assert!(cur_minter.contains(&alice_raw));
        assert!(cur_minter.contains(&bob_raw));
        assert!(cur_minter.contains(&charlie_raw));
        assert!(cur_minter.contains(&admin_raw));

        // let's give it an empty list to see if it breaks
        let minters = vec![];
        let handle_msg = HandleMsg::RemoveMinters {
            minters,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify it is the same list
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 4);
        assert!(cur_minter.contains(&alice_raw));
        assert!(cur_minter.contains(&bob_raw));
        assert!(cur_minter.contains(&charlie_raw));
        assert!(cur_minter.contains(&admin_raw));

        // let's throw some repeats to see if it breaks
        let minters = vec![
            HumanAddr("alice".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
            HumanAddr("charlie".to_string()),
        ];
        let handle_msg = HandleMsg::RemoveMinters {
            minters,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the minters were removed
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 1);
        assert!(!cur_minter.contains(&alice_raw));
        assert!(!cur_minter.contains(&bob_raw));
        assert!(!cur_minter.contains(&charlie_raw));
        assert!(cur_minter.contains(&admin_raw));

        // let's remove the last one
        let handle_msg = HandleMsg::RemoveMinters {
            minters: vec![HumanAddr("admin".to_string())],
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the minters were removed
        let cur_minter: Option<Vec<CanonicalAddr>> = may_load(&deps.storage, MINTERS_KEY).unwrap();
        assert!(cur_minter.is_none());
    }

    // test set minters
    #[test]
    fn test_set_minters() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test setting minters when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let minters = vec![
            HumanAddr("alice".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("charlie".to_string()),
            HumanAddr("bob".to_string()),
            HumanAddr("alice".to_string()),
        ];
        let handle_msg = HandleMsg::SetMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still set minters when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test non admin trying to set minters
        let handle_msg = HandleMsg::SetMinters {
            minters: minters.clone(),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("This is an admin command and can only be run from the admin address")
        );

        // sanity check
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let handle_msg = HandleMsg::SetMinters {
            minters: minters.clone(),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the new minters were added
        let cur_minter: Vec<CanonicalAddr> = load(&deps.storage, MINTERS_KEY).unwrap();
        assert_eq!(cur_minter.len(), 3);
        assert!(cur_minter.contains(&alice_raw));
        assert!(cur_minter.contains(&bob_raw));
        assert!(cur_minter.contains(&charlie_raw));
        // let's try an empty list
        let minters = vec![];
        let handle_msg = HandleMsg::SetMinters {
            minters,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify the minters were removed
        let cur_minter: Option<Vec<CanonicalAddr>> = may_load(&deps.storage, MINTERS_KEY).unwrap();
        assert!(cur_minter.is_none());
    }

    // test change admin
    #[test]
    fn test_change_admin() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test changing admin when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("alice".to_string()),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still change admin when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test non admin trying to change admin
        let handle_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("alice".to_string()),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("This is an admin command and can only be run from the admin address")
        );

        // sanity check
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let admin_raw = deps
            .api
            .canonical_address(&HumanAddr("admin".to_string()))
            .unwrap();
        // verify admin is the current admin
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.admin, admin_raw);
        // change it to alice
        let handle_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("alice".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify admin was changed
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.admin, alice_raw);
    }

    // test set contract status
    #[test]
    fn test_set_contract_status() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test non admin trying to change status
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(
            error.contains("This is an admin command and can only be run from the admin address")
        );

        // sanity check
        // verify current status is normal
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.status, ContractStatus::Normal.to_u8());
        // change it to StopAll
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        // verify status was changed
        let config: Config = load(&deps.storage, CONFIG_KEY).unwrap();
        assert_eq!(config.status, ContractStatus::StopAll.to_u8());
    }

    // test approve_all from the cw721 spec
    #[test]
    fn test_cw721_approve_all() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg); // test burn when status prevents it
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test trying to ApproveAll when status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));
        // setting approval is ok even during StopTransactions status
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let nft1_key = 0u32.to_le_bytes();
        let nft2_key = 1u32.to_le_bytes();
        let nft3_key = 2u32.to_le_bytes();

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);

        // confirm bob has transfer token permissions but not transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm NFT1 permission has bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT2 permission has bob
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT3 permission has bob
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm AuthLists has bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 3);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // test that ApproveAll will remove all the token permissions
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm bob has transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_oper_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm bob's NFT1 permission is gone
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm bob's NFT2 permission is gone
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm bob's NFT3 permission is gone
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm AuthLists no longer have bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());
    }

    // test revoke_all from the cw721 spec
    #[test]
    fn test_cw721_revoke_all() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg); // test burn when status prevents it
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT3".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: None,
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test trying to RevokeAll when status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::RevokeAll {
            operator: HumanAddr("bob".to_string()),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));
        // setting approval is ok even during StopTransactions status
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let nft1_key = 0u32.to_le_bytes();
        let nft2_key = 1u32.to_le_bytes();
        let nft3_key = 2u32.to_le_bytes();

        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT3".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);

        // confirm bob has transfer token permissions but not transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm NFT1 permission has bob
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT2 permission has bob
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm NFT3 permission has bob
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // confirm AuthLists has bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 3);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&1u32));
        assert!(bob_auth.tokens[transfer_idx].contains(&2u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());

        // test that RevokeAll will remove all the token permissions
        let handle_msg = HandleMsg::RevokeAll {
            operator: HumanAddr("bob".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm bob does not have transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        // confirm bob's NFT1 permission is gone
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm bob's NFT2 permission is gone
        let token: Token = json_load(&info_store, &nft2_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm bob's NFT3 permission is gone
        let token: Token = json_load(&info_store, &nft3_key).unwrap();
        assert!(token.permissions.is_empty());
        // confirm AuthLists no longer have bob
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Option<Vec<AuthList>> = may_load(&auth_store, alice_key).unwrap();
        assert!(auth_list.is_none());

        // grant bob transfer all permission to test if revoke all removes it
        let handle_msg = HandleMsg::ApproveAll {
            operator: HumanAddr("bob".to_string()),
            expires: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm bob has transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(
            bob_oper_perm.expirations[transfer_idx],
            Some(Expiration::Never)
        );
        // now get rid of it
        let handle_msg = HandleMsg::RevokeAll {
            operator: HumanAddr("bob".to_string()),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm bob no longer has transfer all permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::Never)
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
    }

    // test set ownership privacy
    #[test]
    fn test_set_ownership_privacy() {
        let (init_result, mut deps) = init_helper_default();
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test setting privacy when status prevents it
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: true,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));

        // you can still set privacy when transactions are stopped
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // sanity check when contract default is private
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: true,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pub_priv: bool = load(&store, alice_key).unwrap();
        assert!(pub_priv);
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: false,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pub_priv: Option<bool> = may_load(&store, alice_key).unwrap();
        assert!(pub_priv.is_none());

        // test when contract default is public
        let (init_result, mut deps) =
            init_helper_with_config(false, true, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: true,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pub_priv: Option<bool> = may_load(&store, alice_key).unwrap();
        assert!(pub_priv.is_none());
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: false,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pub_priv: bool = load(&store, alice_key).unwrap();
        assert!(!pub_priv);
    }

    // test owner setting global approvals
    #[test]
    fn test_set_global_approval() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is public
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("Token ID: NFT1 not found"));

        let (init_result, mut deps) =
            init_helper_with_config(false, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        // test token does not exist when supply is private
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token NFT1"));

        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Pub 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
 
        // test trying to set approval when status does not allow
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopAll,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("The contract admin has temporarily disabled this action"));
        // setting approval is ok even during StopTransactions status
        let handle_msg = HandleMsg::SetContractStatus {
            level: ContractStatus::StopTransactions,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // only allow the owner to use SetGlobalApproval
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("bob", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains("You do not own token NFT1"));

        // try approving a token without specifying which token
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: None,
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains(
            "Attempted to grant/revoke permission for a token, but did not specify a token ID"
        ));

        // try revoking a token approval without specifying which token
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: None,
            view_owner: Some(AccessLevel::RevokeToken),
            view_private_metadata: None,
            expires: None,
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let error = extract_error_msg(handle_result);
        assert!(error.contains(
            "Attempted to grant/revoke permission for a token, but did not specify a token ID"
        ));

        // sanity check
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let global_raw = CanonicalAddr(Binary::from(b"public"));
        let global_key = global_raw.as_slice();
        let alice_raw = deps
            .api
            .canonical_address(&HumanAddr("alice".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 1);
        let global_perm = all_perm.iter().find(|p| p.address == global_raw).unwrap();
        assert_eq!(
            global_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_perm.expirations[view_meta_idx], None);
        assert_eq!(global_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions and that the token data did not get modified
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let nft1_key = 0u32.to_le_bytes();
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.owner, alice_raw);
        assert!(token.unwrapped);
        let pub_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
        let pub_meta: Metadata = load(&pub_store, &nft1_key).unwrap();
        assert_eq!(pub_meta.name, Some("My1".to_string()));
        assert_eq!(pub_meta.description, Some("Pub 1".to_string()));
        assert_eq!(pub_meta.image, Some("URI 1".to_string()));
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
        let priv_meta: Option<Metadata> = may_load(&priv_store, &nft1_key).unwrap();
        assert!(priv_meta.is_none());
        assert_eq!(token.permissions.len(), 1);
        let global_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == global_raw)
            .unwrap();
        assert_eq!(
            global_tok_perm.expirations[view_meta_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_tok_perm.expirations[transfer_idx], None);
        assert_eq!(global_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists has public with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let global_auth = auth_list.iter().find(|a| a.address == global_raw).unwrap();
        assert_eq!(global_auth.tokens[view_meta_idx].len(), 1);
        assert!(global_auth.tokens[view_meta_idx].contains(&0u32));
        assert!(global_auth.tokens[transfer_idx].is_empty());
        assert!(global_auth.tokens[view_owner_idx].is_empty());

        // bob approvals to make sure whitelisted addresses don't break
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        let global_perm = all_perm.iter().find(|p| p.address == global_raw).unwrap();
        assert_eq!(
            global_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_perm.expirations[view_meta_idx], None);
        assert_eq!(global_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(
            global_tok_perm.expirations[view_meta_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_tok_perm.expirations[transfer_idx], None);
        assert_eq!(global_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists has bob with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let global_auth = auth_list.iter().find(|a| a.address == global_raw).unwrap();
        assert_eq!(global_auth.tokens[view_meta_idx].len(), 1);
        assert!(global_auth.tokens[view_meta_idx].contains(&0u32));
        assert!(global_auth.tokens[transfer_idx].is_empty());
        assert!(global_auth.tokens[view_owner_idx].is_empty());

        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        let global_perm = all_perm.iter().find(|p| p.address == global_raw).unwrap();
        assert_eq!(
            global_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_perm.expirations[view_meta_idx], None);
        assert_eq!(global_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 2);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        assert_eq!(
            global_tok_perm.expirations[view_meta_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_tok_perm.expirations[transfer_idx], None);
        assert_eq!(global_tok_perm.expirations[view_owner_idx], None);
        // confirm AuthLists has bob with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 2);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let global_auth = auth_list.iter().find(|a| a.address == global_raw).unwrap();
        assert_eq!(global_auth.tokens[view_meta_idx].len(), 1);
        assert!(global_auth.tokens[view_meta_idx].contains(&0u32));
        assert!(global_auth.tokens[transfer_idx].is_empty());
        assert!(global_auth.tokens[view_owner_idx].is_empty());
    
        // test revoking global approval
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: Some(AccessLevel::None),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        // confirm ALL permission
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let all_perm: Vec<Permission> = json_load(&all_store, alice_key).unwrap();
        assert_eq!(all_perm.len(), 2);
        let bob_oper_perm = all_perm.iter().find(|p| p.address == bob_raw).unwrap();
        assert_eq!(
            bob_oper_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_oper_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_oper_perm.expirations[transfer_idx], None);
        let global_perm = all_perm.iter().find(|p| p.address == global_raw).unwrap();
        assert_eq!(
            global_perm.expirations[view_owner_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(global_perm.expirations[view_meta_idx], None);
        assert_eq!(global_perm.expirations[transfer_idx], None);
        // confirm NFT1 permissions
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token: Token = json_load(&info_store, &nft1_key).unwrap();
        assert_eq!(token.permissions.len(), 1);
        let bob_tok_perm = token
            .permissions
            .iter()
            .find(|p| p.address == bob_raw)
            .unwrap();
        assert_eq!(
            bob_tok_perm.expirations[transfer_idx],
            Some(Expiration::AtTime(1000000))
        );
        assert_eq!(bob_tok_perm.expirations[view_meta_idx], None);
        assert_eq!(bob_tok_perm.expirations[view_owner_idx], None);
        let global_tok_perm = token
        .permissions
        .iter()
        .find(|p| p.address == global_raw);
        assert!(global_tok_perm.is_none());
        // confirm AuthLists has bob with NFT1 permission
        let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
        let auth_list: Vec<AuthList> = load(&auth_store, alice_key).unwrap();
        assert_eq!(auth_list.len(), 1);
        let bob_auth = auth_list.iter().find(|a| a.address == bob_raw).unwrap();
        assert_eq!(bob_auth.tokens[transfer_idx].len(), 1);
        assert!(bob_auth.tokens[transfer_idx].contains(&0u32));
        assert!(bob_auth.tokens[view_meta_idx].is_empty());
        assert!(bob_auth.tokens[view_owner_idx].is_empty());
        let global_auth = auth_list.iter().find(|a| a.address == global_raw);
        assert!(global_auth.is_none());
    }

    // test permissioning works
    #[test]
    fn test_check_permission() {
        let (init_result, mut deps) =
            init_helper_with_config(true, false, false, false, false, false, false);
        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );
        let block = BlockInfo {
            height: 1,
            time: 1,
            chain_id: "secret-2".to_string(),
        };
        let alice_raw = deps
        .api
        .canonical_address(&HumanAddr("alice".to_string()))
        .unwrap();
        let bob_raw = deps
            .api
            .canonical_address(&HumanAddr("bob".to_string()))
            .unwrap();
        let charlie_raw = deps
            .api
            .canonical_address(&HumanAddr("charlie".to_string()))
            .unwrap();
        let alice_key = alice_raw.as_slice();
        let view_owner_idx = PermissionType::ViewOwner.to_u8() as usize;
        let view_meta_idx = PermissionType::ViewMetadata.to_u8() as usize;
        let transfer_idx = PermissionType::Transfer.to_u8() as usize;
        let nft1_key = 0u32.to_le_bytes();
        let nft2_key = 1u32.to_le_bytes();
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Pub 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My2".to_string()),
                description: Some("Pub 2".to_string()),
                image: Some("URI 2".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
 
        // test not approved 
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        
        // test owner is public for the contract
        let (init_result, mut deps) =
        init_helper_with_config(true, true, false, false, false, false, false);
    assert!(
        init_result.is_ok(),
        "Init failed: {}",
        init_result.err().unwrap()
    );
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", true);
        assert!(check_perm.is_ok());

        // test owner makes their tokens private when the contract has public ownership
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: false,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", true);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));

        // test owner makes their tokens public when the contract has private ownership
        let (init_result, mut deps) =
        init_helper_with_config(true, false, false, false, false, false, false);
    assert!(
        init_result.is_ok(),
        "Init failed: {}",
        init_result.err().unwrap()
    );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Pub 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My2".to_string()),
                description: Some("Pub 2".to_string()),
                image: Some("URI 2".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
 
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: true,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test global approval on a token
        let handle_msg = HandleMsg::SetOwnershipPrivacy {
            owner_is_public: false,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: None,
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test global approval for all tokens
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token2: Token = json_load(&info_store, &nft2_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: None,
            view_owner: None,
            view_private_metadata: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token2: Token = json_load(&info_store, &nft2_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test those global permissions having expired 
        let block = BlockInfo {
            height: 1,
            time: 2000000,
            chain_id: "secret-2".to_string(),
        };
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));

        let block = BlockInfo {
            height: 1,
            time: 1,
            chain_id: "secret-2".to_string(),
        };

        // test whitelisted approval on a token 
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT2".to_string()),
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(5)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token2: Token = json_load(&info_store, &nft2_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&bob_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&charlie_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));

        // test approval expired 
        let block = BlockInfo {
            height: 1,
            time: 6,
            chain_id: "secret-2".to_string(),
        };
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&bob_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("Access to token NFT2 has expired"));

        // test owner access
        let check_perm = check_permission(&deps, &block, &token2, "NFT2", Some(&alice_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test whitelisted approval on all tokens 
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(7)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let token2: Token = json_load(&info_store, &nft2_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&charlie_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test whitelisted ALL permission has expired
        let block = BlockInfo {
            height: 1,
            time: 7,
            chain_id: "secret-2".to_string(),
        };
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&charlie_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("Access to all tokens of alice has expired"));

        let (init_result, mut deps) =
        init_helper_default();
    assert!(
        init_result.is_ok(),
        "Init failed: {}",
        init_result.err().unwrap()
    );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Pub 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My2".to_string()),
                description: Some("Pub 2".to_string()),
                image: Some("URI 2".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test whitelist approval expired, but global is good on a token
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: Some(Expiration::AtTime(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(1000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let block = BlockInfo {
            height: 1,
            time: 100,
            chain_id: "secret-2".to_string(),
        };
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test whitelist approval expired, but global is good on ALL tokens
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: Some(AccessLevel::All),
            transfer: None,
            expires: Some(Expiration::AtTime(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: None,
            view_owner: None,
            view_private_metadata: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let block = BlockInfo {
            height: 1,
            time: 100,
            chain_id: "secret-2".to_string(),
        };
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test whitelist approval is good, but global expired on a token
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: Some(Expiration::AtTime(1000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let block = BlockInfo {
            height: 1,
            time: 100,
            chain_id: "secret-2".to_string(),
        };
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // test whitelist approval is good, but global expired on ALL tokens
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: Some(AccessLevel::All),
            transfer: None,
            expires: Some(Expiration::AtTime(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: None,
            view_owner: None,
            view_private_metadata: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(1000)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let block = BlockInfo {
            height: 1,
            time: 100,
            chain_id: "secret-2".to_string(),
        };
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        let (init_result, mut deps) =
        init_helper_default();
    assert!(
        init_result.is_ok(),
        "Init failed: {}",
        init_result.err().unwrap()
    );
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT1".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My1".to_string()),
                description: Some("Pub 1".to_string()),
                image: Some("URI 1".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);
        let handle_msg = HandleMsg::Mint {
            token_id: Some("NFT2".to_string()),
            owner: Some(HumanAddr("alice".to_string())),
            public_metadata: Some(Metadata {
                name: Some("My2".to_string()),
                description: Some("Pub 2".to_string()),
                image: Some("URI 2".to_string()),
            }),
            private_metadata: None,
            memo: None,
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("admin", &[]), handle_msg);

        // test bob has view owner approval on NFT1 and view metadata approval on ALL
        // while there is global view owner approval on ALL tokens and global view metadata
        // approval on NFT1
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("bob".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::ApproveToken),
            view_private_metadata: Some(AccessLevel::All),
            transfer: None,
            expires: Some(Expiration::AtTime(100)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetGlobalApproval {
            token_id: Some("NFT1".to_string()),
            view_owner: Some(AccessLevel::All),
            view_private_metadata: Some(AccessLevel::ApproveToken),
            expires: Some(Expiration::AtTime(10)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let block = BlockInfo {
            height: 1,
            time: 1,
            chain_id: "secret-2".to_string(),
        };
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        let error = extract_error_msg(check_perm);
        assert!(error.contains("not approved"));
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // now check where the global approvals expired 
        let block = BlockInfo {
            height: 1,
            time: 50,
            chain_id: "secret-2".to_string(),
        };
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewOwner, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&bob_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());

        // throw a charlie transfer approval and a view meta token approval in the mix
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: None,
            view_owner: None,
            view_private_metadata: None,
            transfer: Some(AccessLevel::All),
            expires: Some(Expiration::AtTime(100)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let handle_msg = HandleMsg::SetWhitelistedApproval {
            address: HumanAddr("charlie".to_string()),
            token_id: Some("NFT1".to_string()),
            view_owner: None,
            view_private_metadata: Some(AccessLevel::ApproveToken),
            transfer: None,
            expires: Some(Expiration::AtTime(100)),
            padding: None,
        };
        let _handle_result = handle(&mut deps, mock_env("alice", &[]), handle_msg);
        let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
        let token1: Token = json_load(&info_store, &nft1_key).unwrap();    
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&charlie_raw), PermissionType::Transfer, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());
        let check_perm = check_permission(&deps, &block, &token1, "NFT1", Some(&charlie_raw), PermissionType::ViewMetadata, &mut Vec::new(), "not approved", false);
        assert!(check_perm.is_ok());
    }
}
