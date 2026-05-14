# PersistImportedDeploymentRequestNetworkUnion


## Supported Types

### `models.PersistImportedDeploymentRequestNetworkUseDefault`

```typescript
const value: models.PersistImportedDeploymentRequestNetworkUseDefault = {
  type: "use-default",
};
```

### `models.PersistImportedDeploymentRequestNetworkCreate`

```typescript
const value: models.PersistImportedDeploymentRequestNetworkCreate = {
  type: "create",
};
```

### `models.PersistImportedDeploymentRequestNetworkByoVpcAws`

```typescript
const value: models.PersistImportedDeploymentRequestNetworkByoVpcAws = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.PersistImportedDeploymentRequestNetworkByoVpcGcp`

```typescript
const value: models.PersistImportedDeploymentRequestNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.PersistImportedDeploymentRequestNetworkByoVnetAzure`

```typescript
const value: models.PersistImportedDeploymentRequestNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

