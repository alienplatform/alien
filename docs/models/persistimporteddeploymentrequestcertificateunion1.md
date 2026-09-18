# PersistImportedDeploymentRequestCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.PersistImportedDeploymentRequestCertificateTLSSecretRef1`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.PersistImportedDeploymentRequestCertificateManagedAcmImport1`

```typescript
const value:
  models.PersistImportedDeploymentRequestCertificateManagedAcmImport1 = {
    mode: "managedAcmImport",
  };
```

### `models.PersistImportedDeploymentRequestCertificateAwsAcmArn1`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.PersistImportedDeploymentRequestCertificateManagedTLSSecret1`

```typescript
const value:
  models.PersistImportedDeploymentRequestCertificateManagedTLSSecret1 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.PersistImportedDeploymentRequestCertificateNone1`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateNone1 = {
  mode: "none",
};
```

