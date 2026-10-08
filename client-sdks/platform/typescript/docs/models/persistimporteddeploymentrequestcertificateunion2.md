# PersistImportedDeploymentRequestCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.PersistImportedDeploymentRequestCertificateTLSSecretRef2`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.PersistImportedDeploymentRequestCertificateManagedAcmImport2`

```typescript
const value:
  models.PersistImportedDeploymentRequestCertificateManagedAcmImport2 = {
    mode: "managedAcmImport",
  };
```

### `models.PersistImportedDeploymentRequestCertificateAwsAcmArn2`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.PersistImportedDeploymentRequestCertificateManagedTLSSecret2`

```typescript
const value:
  models.PersistImportedDeploymentRequestCertificateManagedTLSSecret2 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.PersistImportedDeploymentRequestCertificateNone2`

```typescript
const value: models.PersistImportedDeploymentRequestCertificateNone2 = {
  mode: "none",
};
```

