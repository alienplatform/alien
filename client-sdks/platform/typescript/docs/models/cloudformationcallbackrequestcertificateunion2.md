# CloudFormationCallbackRequestCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CloudFormationCallbackRequestCertificateTLSSecretRef2`

```typescript
const value: models.CloudFormationCallbackRequestCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CloudFormationCallbackRequestCertificateManagedAcmImport2`

```typescript
const value: models.CloudFormationCallbackRequestCertificateManagedAcmImport2 =
  {
    mode: "managedAcmImport",
  };
```

### `models.CloudFormationCallbackRequestCertificateAwsAcmArn2`

```typescript
const value: models.CloudFormationCallbackRequestCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CloudFormationCallbackRequestCertificateManagedTLSSecret2`

```typescript
const value: models.CloudFormationCallbackRequestCertificateManagedTLSSecret2 =
  {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.CloudFormationCallbackRequestCertificateNone2`

```typescript
const value: models.CloudFormationCallbackRequestCertificateNone2 = {
  mode: "none",
};
```

