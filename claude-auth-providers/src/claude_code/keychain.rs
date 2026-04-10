use std::{collections::HashSet, ptr};

use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionary},
    string::CFString,
};
use security_framework::{
    base::Result as SFResult,
    passwords::{PasswordOptions, generic_password},
};
use security_framework_sys::{
    item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecMatchLimit,
        kSecMatchLimitAll, kSecReturnAttributes,
    },
    keychain_item::SecItemCopyMatching,
};

use crate::{
    Error,
    claude_code::credential::{ClaudeCredential, parse_credentials},
};

const PRIMARY_SERVICE: &str = "Claude Code-credentials";
const SERVICE_PREFIX: &str = "Claude Code-credentials-";

pub fn get_credentials() -> Result<Vec<ClaudeCredential>, Error> {
    Ok(list_all_credentials()?
        .into_iter()
        .filter_map(|(acct, svc)| read_credential(&acct, &svc).transpose())
        .filter_map(|res| res.map(|s| parse_credentials(&s)).transpose())
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_credential(acct: &str, svc: &str) -> SFResult<Option<String>> {
    let password = generic_password(PasswordOptions::new_generic_password(svc, acct))?;
    Ok(String::from_utf8(password).ok())
}

/// Lists all credentials (acct, svc) from the keychain that may be Claude Code credentials.
fn list_all_credentials() -> SFResult<Vec<(String, String)>> {
    let query = build_query();

    let mut result = ptr::null();
    unsafe {
        cvt(SecItemCopyMatching(
            query.as_concrete_TypeRef(),
            &raw mut result,
        ))?;
    }
    if result.is_null() {
        return Ok(vec![]);
    }

    // With kSecMatchLimitAll + kSecReturnAttributes=true, expect an array of dictionaries.
    let array = unsafe { CFArray::<CFType>::wrap_under_create_rule(result.cast()) };

    let mut claude_svcs = HashSet::new();

    for i in 0..array.len() {
        let item = array.get(i).expect("i should be in range of array");

        let dict_ref: CFDictionaryRef = item.as_CFTypeRef().cast();
        if dict_ref.is_null() {
            continue;
        }

        let dict = unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(dict_ref) };

        let svc_key = unsafe { CFString::wrap_under_get_rule(kSecAttrService) };
        let acct_key = unsafe { CFString::wrap_under_get_rule(kSecAttrAccount) };
        if let Some(svc) = dict.find(&svc_key)
            && let Some(acct) = dict.find(&acct_key)
        {
            let svc_ref = svc.as_CFTypeRef();
            if svc_ref.is_null() {
                continue;
            }

            let acct_ref = acct.as_CFTypeRef();
            if acct_ref.is_null() {
                continue;
            }

            let svc = unsafe { CFString::wrap_under_get_rule(svc_ref.cast()) }.to_string();
            let acct = unsafe { CFString::wrap_under_get_rule(acct_ref.cast()) }.to_string();
            let key = (acct, svc);
            if is_claude_code_credential(&key.1) && !claude_svcs.contains(&key) {
                claude_svcs.insert(key);
            }
        }
    }

    Ok(claude_svcs.into_iter().collect())
}

fn build_query() -> CFDictionary<CFString, CFType> {
    let mut query = CFMutableDictionary::new();

    query.add(
        &unsafe { CFString::wrap_under_get_rule(kSecClass) },
        &unsafe { CFType::wrap_under_get_rule(kSecClassGenericPassword.cast()) },
    );

    query.add(
        &unsafe { CFString::wrap_under_get_rule(kSecMatchLimit.cast()) },
        &unsafe { CFType::wrap_under_get_rule(kSecMatchLimitAll.cast()) },
    );
    query.add(
        &unsafe { CFString::wrap_under_get_rule(kSecReturnAttributes.cast()) },
        &CFBoolean::true_value().as_CFType(),
    );

    query.to_immutable()
}

fn is_claude_code_credential(svc: &str) -> bool {
    if svc == PRIMARY_SERVICE {
        return true;
    }

    svc.strip_prefix(SERVICE_PREFIX)
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn cvt(status: i32) -> SFResult<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(security_framework::base::Error::from_code(status))
    }
}

#[cfg(test)]
#[allow(unused)]
mod tests {
    use super::*;

    /// Temporary test credential stored in keychain.
    /// Deleted on drop.
    struct TestCredential {
        acct: String,
        svc: String,
    }

    impl Drop for TestCredential {
        fn drop(&mut self) {
            let _ = security_framework::passwords::delete_generic_password(&self.svc, &self.acct);
        }
    }

    impl TestCredential {
        pub fn new(acct: &str, svc: &str, value: &str) -> Self {
            security_framework::passwords::set_generic_password(svc, acct, value.as_bytes())
                .expect("failed to add test credential to keychain");
            Self {
                acct: acct.to_owned(),
                svc: svc.to_owned(),
            }
        }
    }

    #[test]
    fn test_list_all_credentials() {
        let x = list_all_credentials().unwrap();
        println!("{x:?}");
    }
}
