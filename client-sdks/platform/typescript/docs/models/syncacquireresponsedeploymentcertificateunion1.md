# SyncAcquireResponseDeploymentCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncAcquireResponseDeploymentCertificateTLSSecretRef1`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncAcquireResponseDeploymentCertificateManagedAcmImport1`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateManagedAcmImport1 =
  {
    mode: "managedAcmImport",
  };
```

### `models.SyncAcquireResponseDeploymentCertificateAwsAcmArn1`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncAcquireResponseDeploymentCertificateManagedTLSSecret1`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateManagedTLSSecret1 =
  {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.SyncAcquireResponseDeploymentCertificateNone1`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateNone1 = {
  mode: "none",
};
```

