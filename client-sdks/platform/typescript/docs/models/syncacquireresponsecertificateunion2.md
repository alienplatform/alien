# SyncAcquireResponseCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncAcquireResponseCertificateTLSSecretRef2`

```typescript
const value: models.SyncAcquireResponseCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncAcquireResponseCertificateManagedAcmImport2`

```typescript
const value: models.SyncAcquireResponseCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.SyncAcquireResponseCertificateAwsAcmArn2`

```typescript
const value: models.SyncAcquireResponseCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncAcquireResponseCertificateManagedTLSSecret2`

```typescript
const value: models.SyncAcquireResponseCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncAcquireResponseCertificateNone2`

```typescript
const value: models.SyncAcquireResponseCertificateNone2 = {
  mode: "none",
};
```

