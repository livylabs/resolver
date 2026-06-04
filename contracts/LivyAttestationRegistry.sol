// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title LivyAttestationRegistry
/// @notice Gas-conscious on-chain registry for Livy attestations already
/// written to the provenance layer.
/// @dev Full attestation material and human-readable references stay off-chain.
/// The contract stores only compact anchors; events carry the Arweave and Livy
/// Explorer references for indexers. Authorized registrars assert that the
/// provenance layer already performed verification before registration.
contract LivyAttestationRegistry {
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

    struct AttestationAnchor {
        bytes32 anchorDigest;
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
    error EmptyProvenanceAttestationId();
    error EmptyLivyExplorerId();
    error EmptyArweaveLocation();
    error ZeroPublicValuesCommitment();
    error ZeroReportPayloadHash();
    error ZeroScopeHash();
    error ZeroAttestationClaimHash();
    error ZeroSubjectHash();
    error AttestationAlreadyRegistered(bytes32 attestationKey);
    error UnknownAttestation(bytes32 attestationKey);

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event RegistrarUpdated(address indexed registrar, bool allowed);
    event AttestationRegistered(
        bytes32 indexed attestationKey,
        bytes32 indexed publicValuesCommitment,
        bytes32 indexed anchorDigest,
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

    address public owner;

    mapping(address registrar => bool allowed) public registrars;

    mapping(bytes32 attestationKey => AttestationAnchor anchor) private _anchors;

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    modifier onlyRegistrar() {
        if (!registrars[msg.sender]) revert NotRegistrar();
        _;
    }

    constructor(address initialOwner) {
        if (initialOwner == address(0)) revert OwnerCannotBeZero();

        owner = initialOwner;
        registrars[initialOwner] = true;

        emit OwnershipTransferred(address(0), initialOwner);
        emit RegistrarUpdated(initialOwner, true);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert OwnerCannotBeZero();

        address previousOwner = owner;
        owner = newOwner;

        emit OwnershipTransferred(previousOwner, newOwner);
    }

    function setRegistrar(address registrar, bool allowed) external onlyOwner {
        registrars[registrar] = allowed;

        emit RegistrarUpdated(registrar, allowed);
    }

    function register(RegisterInput calldata input) external onlyRegistrar returns (bytes32 attestationKey) {
        _validate(input);

        attestationKey = attestationKeyOf(input.provenanceAttestationId);
        if (_anchors[attestationKey].registeredAt != 0) revert AttestationAlreadyRegistered(attestationKey);

        bytes32 referenceHash = referenceHashOf(input);
        bytes32 contextHash = contextHashOf(input);
        bytes32 anchorDigest = _anchorDigest(
            referenceHash, input.publicValuesCommitment, input.reportPayloadHash, input.reportNonce, contextHash
        );

        _anchors[attestationKey] = AttestationAnchor({
            anchorDigest: anchorDigest,
            publicValuesCommitment: input.publicValuesCommitment,
            reportPayloadHash: input.reportPayloadHash,
            referenceHash: referenceHash,
            reportNonce: input.reportNonce,
            registeredAt: uint64(block.timestamp),
            schemaBindingStatus: input.schemaBindingStatus
        });

        emit AttestationRegistered({
            attestationKey: attestationKey,
            publicValuesCommitment: input.publicValuesCommitment,
            anchorDigest: anchorDigest,
            reportPayloadHash: input.reportPayloadHash,
            reportNonce: input.reportNonce,
            referenceHash: referenceHash,
            contextHash: contextHash,
            schemaBindingStatus: input.schemaBindingStatus,
            registrar: msg.sender
        });
        emit AttestationReferences({
            attestationKey: attestationKey,
            provenanceAttestationId: input.provenanceAttestationId,
            livyExplorerId: input.livyExplorerId,
            arweaveLocation: input.arweaveLocation
        });
    }

    function getAttestation(bytes32 attestationKey) external view returns (AttestationAnchor memory) {
        AttestationAnchor memory anchor = _anchors[attestationKey];
        if (anchor.registeredAt == 0) revert UnknownAttestation(attestationKey);

        return anchor;
    }

    function getAttestationByProvenanceId(string calldata provenanceAttestationId)
        external
        view
        returns (AttestationAnchor memory)
    {
        bytes32 attestationKey = attestationKeyOf(provenanceAttestationId);
        AttestationAnchor memory anchor = _anchors[attestationKey];
        if (anchor.registeredAt == 0) revert UnknownAttestation(attestationKey);

        return anchor;
    }

    function isRegistered(bytes32 attestationKey) external view returns (bool) {
        return _anchors[attestationKey].registeredAt != 0;
    }

    function attestationKeyOf(string calldata provenanceAttestationId) public pure returns (bytes32) {
        return keccak256(bytes(provenanceAttestationId));
    }

    function referenceHashOf(RegisterInput calldata input) public pure returns (bytes32) {
        return keccak256(abi.encode(input.provenanceAttestationId, input.livyExplorerId, input.arweaveLocation));
    }

    function contextHashOf(RegisterInput calldata input) public pure returns (bytes32) {
        return keccak256(
            abi.encode(
                input.scopeHash,
                input.attestationClaimHash,
                input.subjectHash,
                input.schemaHash,
                input.schemaBindingStatus
            )
        );
    }

    function anchorDigestOf(RegisterInput calldata input) public view returns (bytes32) {
        return _anchorDigest(
            referenceHashOf(input),
            input.publicValuesCommitment,
            input.reportPayloadHash,
            input.reportNonce,
            contextHashOf(input)
        );
    }

    function _anchorDigest(
        bytes32 referenceHash,
        bytes32 publicValuesCommitment,
        bytes32 reportPayloadHash,
        uint64 reportNonce,
        bytes32 contextHash
    ) private view returns (bytes32) {
        return keccak256(
            abi.encode(
                block.chainid,
                address(this),
                referenceHash,
                publicValuesCommitment,
                reportPayloadHash,
                reportNonce,
                contextHash
            )
        );
    }

    function _validate(RegisterInput calldata input) private pure {
        if (bytes(input.provenanceAttestationId).length == 0) revert EmptyProvenanceAttestationId();
        if (bytes(input.livyExplorerId).length == 0) revert EmptyLivyExplorerId();
        if (bytes(input.arweaveLocation).length == 0) revert EmptyArweaveLocation();
        if (input.publicValuesCommitment == bytes32(0)) revert ZeroPublicValuesCommitment();
        if (input.reportPayloadHash == bytes32(0)) revert ZeroReportPayloadHash();
        if (input.scopeHash == bytes32(0)) revert ZeroScopeHash();
        if (input.attestationClaimHash == bytes32(0)) revert ZeroAttestationClaimHash();
        if (input.subjectHash == bytes32(0)) revert ZeroSubjectHash();
    }
}
