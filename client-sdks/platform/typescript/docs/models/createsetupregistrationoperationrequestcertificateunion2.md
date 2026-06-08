# CreateSetupRegistrationOperationRequestCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateSetupRegistrationOperationRequestCertificateTLSSecretRef2`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateTLSSecretRef2 = {
    secretName: "<value>",
    mode: "tlsSecretRef",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateManagedAcmImport2`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateManagedAcmImport2 = {
    mode: "managedAcmImport",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateAwsAcmArn2`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateAwsAcmArn2 = {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret2`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestCertificateManagedTLSSecret2 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.CreateSetupRegistrationOperationRequestCertificateNone2`

```typescript
const value: models.CreateSetupRegistrationOperationRequestCertificateNone2 = {
  mode: "none",
};
```

