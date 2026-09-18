# DeploymentDetailResponseNetworkUnion


## Supported Types

### `models.DeploymentDetailResponseNetworkUseDefault`

```typescript
const value: models.DeploymentDetailResponseNetworkUseDefault = {
  type: "use-default",
};
```

### `models.DeploymentDetailResponseNetworkCreate`

```typescript
const value: models.DeploymentDetailResponseNetworkCreate = {
  type: "create",
};
```

### `models.DeploymentDetailResponseNetworkByoVpcAws`

```typescript
const value: models.DeploymentDetailResponseNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.DeploymentDetailResponseNetworkByoVpcGcp`

```typescript
const value: models.DeploymentDetailResponseNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.DeploymentDetailResponseNetworkByoVnetAzure`

```typescript
const value: models.DeploymentDetailResponseNetworkByoVnetAzure = {
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

