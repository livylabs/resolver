// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title ILivyAttestationRegistry
/// @notice Shared ABI, types, events, and errors for Livy attestation records.
interface ILivyAttestationRegistry {
    enum SchemaBindingStatus {
        Unbound,
        Partial,
        Full
    }

    struct RegisterInput {
        // Backend provenance_attestations.provenance_attestation_id, encoded as
        // the canonical UUID/string used by Livy APIs and Livy Explorer.
        string provenanceAttestationId;
        // Livy Explorer identifier or path component for the public record.
        string livyExplorerId;
        // Arweave transaction ID, ar:// URI, or HTTPS gateway URL.
        string arweaveLocation;
        // SHA-256(public_values_wire_bytes) stored by the provenance layer.
        bytes32 publicValuesCommitment;
        // ReportData.payload_hash from livy-tee.
        bytes32 reportPayloadHash;
        // ReportData.nonce from livy-tee.
        uint64 reportNonce;
        // keccak256(abi.encode(tenant_id, project_id, integration_id)).
        bytes32 scopeHash;
        // keccak256(bytes(attestation_claim)), for example "source" or "policy".
        bytes32 attestationClaimHash;
        // keccak256(abi.encode(subject_type, subject_id)).
        bytes32 subjectHash;
        // bytes32(0) when unbound, otherwise keccak256(abi.encode(schema_id, schema_version)).
        bytes32 schemaHash;
        SchemaBindingStatus schemaBindingStatus;
    }

    struct AttestationRecord {
        bytes32 recordDigest;
        bytes32 publicValuesCommitment;
        bytes32 reportPayloadHash;
        bytes32 referenceHash;
        uint64 reportNonce;
        uint64 registeredAt;
        SchemaBindingStatus schemaBindingStatus;
    }

    error NotOwner();
    error NotRegistrar();
    error OwnerCannotBeZero();
    error RegistrarCannotBeZero();
    error EmptyProvenanceAttestationId();
    error EmptyLivyExplorerId();
    error EmptyArweaveLocation();
    error ZeroPublicValuesCommitment();
    error ZeroReportPayloadHash();
    error ZeroScopeHash();
    error ZeroAttestationClaimHash();
    error ZeroSubjectHash();
    error MissingSchemaHash();
    error UnexpectedSchemaHash();
    error AttestationAlreadyRegistered(bytes32 attestationKey);
    error UnknownAttestation(bytes32 attestationKey);

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event RegistrarUpdated(address indexed registrar, bool allowed);
    event AttestationRegistered(
        bytes32 indexed attestationKey,
        bytes32 indexed publicValuesCommitment,
        bytes32 indexed recordDigest,
        bytes32 reportPayloadHash,
        uint64 reportNonce,
        bytes32 referenceHash,
        bytes32 contextHash,
        SchemaBindingStatus schemaBindingStatus,
        address registrar
    );
    event AttestationReferences(
        bytes32 indexed attestationKey, string provenanceAttestationId, string livyExplorerId, string arweaveLocation
    );

    function owner() external view returns (address);
    function registrars(address registrar) external view returns (bool);
    function transferOwnership(address newOwner) external;
    function setRegistrar(address registrar, bool allowed) external;
    function register(RegisterInput calldata input) external returns (bytes32 attestationKey);
    function getAttestation(bytes32 attestationKey) external view returns (AttestationRecord memory);
    function getAttestationByProvenanceId(string calldata provenanceAttestationId)
        external
        view
        returns (AttestationRecord memory);
    function isRegistered(bytes32 attestationKey) external view returns (bool);
    function isRegisteredByProvenanceId(string calldata provenanceAttestationId) external view returns (bool);
    function attestationKeyOf(string calldata provenanceAttestationId) external pure returns (bytes32);
    function referenceHashOf(RegisterInput calldata input) external pure returns (bytes32);
    function contextHashOf(RegisterInput calldata input) external pure returns (bytes32);
    function recordDigestOf(RegisterInput calldata input) external view returns (bytes32);
}
