// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ILivyAttestationRegistry} from "../ILivyAttestationRegistry.sol";
import {LivyAttestationRegistry} from "../registry.sol";

contract LivyAttestationRegistryTest {
    function testRegisterStoresValidUnboundRecord() public {
        LivyAttestationRegistry registry = new LivyAttestationRegistry(address(this));
        ILivyAttestationRegistry.RegisterInput memory input = _validInput();

        bytes32 attestationKey = registry.register(input);
        ILivyAttestationRegistry.AttestationRecord memory record = registry.getAttestation(attestationKey);

        require(attestationKey == registry.attestationKeyOf(input.provenanceAttestationId), "wrong attestation key");
        require(registry.isRegistered(attestationKey), "record not registered");
        require(record.recordDigest == registry.recordDigestOf(input), "wrong record digest");
        require(record.publicValuesCommitment == input.publicValuesCommitment, "wrong public values commitment");
        require(record.reportPayloadHash == input.reportPayloadHash, "wrong report payload hash");
        require(record.reportNonce == input.reportNonce, "wrong report nonce");
        require(record.schemaBindingStatus == input.schemaBindingStatus, "wrong schema status");
    }

    function testRequiredFieldsRejectEmptyReferences() public {
        LivyAttestationRegistry registry = new LivyAttestationRegistry(address(this));
        ILivyAttestationRegistry.RegisterInput memory input = _validInput();

        input.provenanceAttestationId = "";
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.EmptyProvenanceAttestationId.selector);

        input = _validInput();
        input.livyExplorerId = "";
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.EmptyLivyExplorerId.selector);

        input = _validInput();
        input.arweaveLocation = "";
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.EmptyArweaveLocation.selector);
    }

    function testRequiredFieldsRejectZeroCommitments() public {
        LivyAttestationRegistry registry = new LivyAttestationRegistry(address(this));
        ILivyAttestationRegistry.RegisterInput memory input = _validInput();

        input.publicValuesCommitment = bytes32(0);
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.ZeroPublicValuesCommitment.selector);

        input = _validInput();
        input.reportPayloadHash = bytes32(0);
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.ZeroReportPayloadHash.selector);

        input = _validInput();
        input.scopeHash = bytes32(0);
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.ZeroScopeHash.selector);

        input = _validInput();
        input.attestationClaimHash = bytes32(0);
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.ZeroAttestationClaimHash.selector);

        input = _validInput();
        input.subjectHash = bytes32(0);
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.ZeroSubjectHash.selector);
    }

    function testRequiredFieldsRejectSchemaMismatch() public {
        LivyAttestationRegistry registry = new LivyAttestationRegistry(address(this));
        ILivyAttestationRegistry.RegisterInput memory input = _validInput();

        input.schemaHash = keccak256("schema:v1");
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.UnexpectedSchemaHash.selector);

        input = _validInput();
        input.schemaBindingStatus = ILivyAttestationRegistry.SchemaBindingStatus.Full;
        _expectRegisterRevert(registry, input, ILivyAttestationRegistry.MissingSchemaHash.selector);

        input.schemaHash = keccak256("schema:v1");
        bytes32 attestationKey = registry.register(input);
        require(registry.isRegistered(attestationKey), "full schema record not registered");
    }

    function _validInput() private pure returns (ILivyAttestationRegistry.RegisterInput memory input) {
        input = ILivyAttestationRegistry.RegisterInput({
            provenanceAttestationId: "prov-attestation-1",
            livyExplorerId: "explorer-record-1",
            arweaveLocation: "ar://example",
            publicValuesCommitment: keccak256("public-values"),
            reportPayloadHash: keccak256("report-payload"),
            reportNonce: 42,
            scopeHash: keccak256("scope"),
            attestationClaimHash: keccak256("source"),
            subjectHash: keccak256("subject"),
            schemaHash: bytes32(0),
            schemaBindingStatus: ILivyAttestationRegistry.SchemaBindingStatus.Unbound
        });
    }

    function _expectRegisterRevert(
        LivyAttestationRegistry registry,
        ILivyAttestationRegistry.RegisterInput memory input,
        bytes4 expectedSelector
    ) private {
        (bool ok, bytes memory revertData) =
            address(registry).call(abi.encodeWithSelector(registry.register.selector, input));

        require(!ok, "register did not revert");
        require(_revertSelector(revertData) == expectedSelector, "wrong revert selector");
    }

    function _revertSelector(bytes memory revertData) private pure returns (bytes4 selector) {
        require(revertData.length >= 4, "missing revert selector");

        assembly {
            selector := mload(add(revertData, 32))
        }
    }
}
