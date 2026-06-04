// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ILivyAttestationRegistry} from "./ILivyAttestationRegistry.sol";

/// @title LivyAttestationRegistry
/// @notice Gas-conscious on-chain registry for Livy attestations already
/// written to the provenance layer.
/// @dev Full attestation material and human-readable references stay off-chain.
/// The contract stores only compact records; events carry the Arweave and Livy
/// Explorer references for indexers. Authorized registrars assert that the
/// provenance layer already performed verification before registration.
contract LivyAttestationRegistry is ILivyAttestationRegistry {
    address public owner;

    mapping(address registrar => bool allowed) public registrars;

    mapping(bytes32 attestationKey => AttestationRecord record) private _records;

    modifier onlyOwner() {
        _checkOwner();
        _;
    }

    modifier onlyRegistrar() {
        _checkRegistrar();
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
        if (registrar == address(0)) revert RegistrarCannotBeZero();

        _setRegistrar(registrar, allowed);
    }

    function register(RegisterInput calldata input) external onlyRegistrar returns (bytes32 attestationKey) {
        _checkRequiredFields(input);

        attestationKey = attestationKeyOf(input.provenanceAttestationId);
        if (_records[attestationKey].registeredAt != 0) revert AttestationAlreadyRegistered(attestationKey);

        bytes32 referenceHash = referenceHashOf(input);
        bytes32 contextHash = contextHashOf(input);
        bytes32 recordDigest = _recordDigest(
            referenceHash, input.publicValuesCommitment, input.reportPayloadHash, input.reportNonce, contextHash
        );

        _records[attestationKey] = AttestationRecord({
            recordDigest: recordDigest,
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
            recordDigest: recordDigest,
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

    function getAttestation(bytes32 attestationKey) external view returns (AttestationRecord memory) {
        return _getRecord(attestationKey);
    }

    function getAttestationByProvenanceId(string calldata provenanceAttestationId)
        external
        view
        returns (AttestationRecord memory)
    {
        return _getRecord(attestationKeyOf(provenanceAttestationId));
    }

    function isRegistered(bytes32 attestationKey) external view returns (bool) {
        return _records[attestationKey].registeredAt != 0;
    }

    function isRegisteredByProvenanceId(string calldata provenanceAttestationId) external view returns (bool) {
        return _records[attestationKeyOf(provenanceAttestationId)].registeredAt != 0;
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

    function recordDigestOf(RegisterInput calldata input) public view returns (bytes32) {
        return _recordDigest(
            referenceHashOf(input),
            input.publicValuesCommitment,
            input.reportPayloadHash,
            input.reportNonce,
            contextHashOf(input)
        );
    }

    function _checkOwner() private view {
        if (msg.sender != owner) revert NotOwner();
    }

    function _checkRegistrar() private view {
        if (!registrars[msg.sender]) revert NotRegistrar();
    }

    function _setRegistrar(address registrar, bool allowed) private {
        registrars[registrar] = allowed;

        emit RegistrarUpdated(registrar, allowed);
    }

    function _getRecord(bytes32 attestationKey) private view returns (AttestationRecord memory record) {
        record = _records[attestationKey];
        if (record.registeredAt == 0) revert UnknownAttestation(attestationKey);
    }

    function _recordDigest(
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

    function _checkRequiredFields(RegisterInput calldata input) private pure {
        if (bytes(input.provenanceAttestationId).length == 0) revert EmptyProvenanceAttestationId();
        if (bytes(input.livyExplorerId).length == 0) revert EmptyLivyExplorerId();
        if (bytes(input.arweaveLocation).length == 0) revert EmptyArweaveLocation();
        if (input.publicValuesCommitment == bytes32(0)) revert ZeroPublicValuesCommitment();
        if (input.reportPayloadHash == bytes32(0)) revert ZeroReportPayloadHash();
        if (input.scopeHash == bytes32(0)) revert ZeroScopeHash();
        if (input.attestationClaimHash == bytes32(0)) revert ZeroAttestationClaimHash();
        if (input.subjectHash == bytes32(0)) revert ZeroSubjectHash();

        if (input.schemaBindingStatus == SchemaBindingStatus.Unbound) {
            if (input.schemaHash != bytes32(0)) revert UnexpectedSchemaHash();
        } else if (input.schemaHash == bytes32(0)) {
            revert MissingSchemaHash();
        }
    }
}
