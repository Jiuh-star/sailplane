//! Role-based access control.
//!
//! Roles are stored on the sailplane user row; capabilities are a bitmask
//! derived from the role. API-key principals bypass every check, matching
//! upstream where possession of the Headscale admin key is the trust boundary.

use serde::{Deserialize, Serialize};

/// A single permission bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(pub u32);

#[allow(non_upper_case_globals)]
impl Capability {
    pub const UiAccess: Capability = Capability(1 << 0);
    pub const ReadPolicy: Capability = Capability(1 << 1);
    pub const WritePolicy: Capability = Capability(1 << 2);
    pub const ReadNetwork: Capability = Capability(1 << 3);
    pub const WriteNetwork: Capability = Capability(1 << 4);
    pub const ReadFeature: Capability = Capability(1 << 5);
    pub const WriteFeature: Capability = Capability(1 << 6);
    pub const ConfigureIam: Capability = Capability(1 << 7);
    pub const ReadMachines: Capability = Capability(1 << 8);
    pub const WriteMachines: Capability = Capability(1 << 9);
    pub const ReadUsers: Capability = Capability(1 << 10);
    pub const WriteUsers: Capability = Capability(1 << 11);
    pub const GenerateAuthKeys: Capability = Capability(1 << 12);
    pub const UseTags: Capability = Capability(1 << 13);
    pub const WriteTailnet: Capability = Capability(1 << 14);
    pub const Owner: Capability = Capability(1 << 15);
    pub const GenerateOwnAuthKeys: Capability = Capability(1 << 16);

    /// All bits set; used by the owner role and API-key principals.
    pub const ALL: Capability = Capability((1 << 17) - 1);
}

impl std::ops::BitOr for Capability {
    type Output = Capability;
    fn bitor(self, rhs: Capability) -> Capability {
        Capability(self.0 | rhs.0)
    }
}

/// The set of capabilities granted to a principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(pub u32);

impl CapabilitySet {
    pub const NONE: CapabilitySet = CapabilitySet(0);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub fn contains(self, capability: Capability) -> bool {
        self.0 & capability.0 == capability.0
    }

    pub fn contains_all(self, capabilities: &[Capability]) -> bool {
        capabilities.iter().all(|c| self.contains(*c))
    }

    pub fn union(self, other: CapabilitySet) -> Self {
        Self(self.0 | other.0)
    }
}

/// A Sailplane account role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    NetworkAdmin,
    ItAdmin,
    Auditor,
    Viewer,
    #[default]
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::NetworkAdmin => "network_admin",
            Self::ItAdmin => "it_admin",
            Self::Auditor => "auditor",
            Self::Viewer => "viewer",
            Self::Member => "member",
        }
    }

    /// Parses a role name, falling back to `member` for unknown values.
    pub fn parse(input: &str) -> Self {
        match input.trim().to_ascii_lowercase().as_str() {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            "network_admin" => Self::NetworkAdmin,
            "it_admin" => Self::ItAdmin,
            "auditor" => Self::Auditor,
            "viewer" => Self::Viewer,
            _ => Self::Member,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::NetworkAdmin => "Network Admin",
            Self::ItAdmin => "IT Admin",
            Self::Auditor => "Auditor",
            Self::Viewer => "Viewer",
            Self::Member => "Member",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Owner => "Full control of Sailplane and everything it manages.",
            Self::Admin => "Manage machines, users, policy and network settings.",
            Self::NetworkAdmin => "Manage ACL policy, DNS and devices, but not Sailplane users.",
            Self::ItAdmin => "Manage users and machines and configure authentication.",
            Self::Auditor => "Read-only access to everything, plus personal auth keys.",
            Self::Viewer => "See machines and users, plus personal auth keys.",
            Self::Member => "No access to the Sailplane UI.",
        }
    }

    /// Every role that can be assigned through the UI (owner is excluded).
    pub fn assignable() -> [Role; 6] {
        [
            Self::Admin,
            Self::NetworkAdmin,
            Self::ItAdmin,
            Self::Auditor,
            Self::Viewer,
            Self::Member,
        ]
    }

    pub fn capabilities(self) -> CapabilitySet {
        use Capability as C;

        match self {
            Self::Owner => CapabilitySet::from_bits(C::ALL.0),
            Self::Admin => CapabilitySet::from_bits(C::ALL.0 & !C::Owner.0 & !C::GenerateOwnAuthKeys.0),
            Self::NetworkAdmin => CapabilitySet::NONE
                .union(C::UiAccess.into())
                .union(C::ReadPolicy.into())
                .union(C::WritePolicy.into())
                .union(C::ReadNetwork.into())
                .union(C::WriteNetwork.into())
                .union(C::ReadFeature.into())
                .union(C::ReadMachines.into())
                .union(C::ReadUsers.into())
                .union(C::GenerateAuthKeys.into())
                .union(C::UseTags.into())
                .union(C::WriteTailnet.into()),
            Self::ItAdmin => CapabilitySet::NONE
                .union(C::UiAccess.into())
                .union(C::ReadPolicy.into())
                .union(C::ReadNetwork.into())
                .union(C::ReadFeature.into())
                .union(C::WriteFeature.into())
                .union(C::ConfigureIam.into())
                .union(C::ReadMachines.into())
                .union(C::WriteMachines.into())
                .union(C::ReadUsers.into())
                .union(C::WriteUsers.into())
                .union(C::GenerateAuthKeys.into()),
            Self::Auditor => CapabilitySet::NONE
                .union(C::UiAccess.into())
                .union(C::ReadPolicy.into())
                .union(C::ReadNetwork.into())
                .union(C::ReadFeature.into())
                .union(C::ReadMachines.into())
                .union(C::ReadUsers.into())
                .union(C::GenerateOwnAuthKeys.into()),
            Self::Viewer => CapabilitySet::NONE
                .union(C::UiAccess.into())
                .union(C::ReadMachines.into())
                .union(C::ReadUsers.into())
                .union(C::GenerateOwnAuthKeys.into()),
            Self::Member => CapabilitySet::NONE,
        }
    }
}

impl From<Capability> for CapabilitySet {
    fn from(capability: Capability) -> Self {
        CapabilitySet(capability.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Capability as C;

    #[test]
    fn admin_cannot_grant_ownership() {
        let admin = Role::Admin.capabilities();
        assert!(!admin.contains(C::Owner));
        assert!(!admin.contains(C::GenerateOwnAuthKeys));
        assert!(admin.contains(C::WriteMachines));
    }

    #[test]
    fn viewer_is_read_only() {
        let viewer = Role::Viewer.capabilities();
        assert!(viewer.contains(C::ReadMachines));
        assert!(!viewer.contains(C::WriteMachines));
        assert!(viewer.contains(C::GenerateOwnAuthKeys));
    }

    #[test]
    fn member_has_no_access() {
        assert_eq!(Role::Member.capabilities(), CapabilitySet::NONE);
    }

    #[test]
    fn unknown_role_names_fall_back_to_member() {
        assert_eq!(Role::parse("superuser"), Role::Member);
        assert_eq!(Role::parse("NETWORK_ADMIN"), Role::NetworkAdmin);
    }
}
