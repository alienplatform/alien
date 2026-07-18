# SyncAcquireResponseDeploymentCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncAcquireResponseDeploymentCertificateTLSSecretRef2`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncAcquireResponseDeploymentCertificateManagedAcmImport2`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateManagedAcmImport2 =
  {
    mode: "managedAcmImport",
  };
```

### `models.SyncAcquireResponseDeploymentCertificateAwsAcmArn2`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncAcquireResponseDeploymentCertificateManagedTLSSecret2`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateManagedTLSSecret2 =
  {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.SyncAcquireResponseDeploymentCertificateNone2`

```typescript
const value: models.SyncAcquireResponseDeploymentCertificateNone2 = {
  mode: "none",
};
```

