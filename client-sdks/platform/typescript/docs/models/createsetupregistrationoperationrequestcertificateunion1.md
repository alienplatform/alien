# CreateSetupRegistrationOperationRequestCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateSetupRegistrationOperationRequestCertificateTLSSecretRef1`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateTLSSecretRef1 = {
    secretName: "<value>",
    mode: "tlsSecretRef",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateManagedAcmImport1`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateManagedAcmImport1 = {
    mode: "managedAcmImport",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateAwsAcmArn1`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateAwsAcmArn1 = {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret1`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret1 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateNone1`

```typescript
const value: models.CreateSetupRegistrationOperationRequestCertificateNone1 = {
  mode: "none",
};
```

