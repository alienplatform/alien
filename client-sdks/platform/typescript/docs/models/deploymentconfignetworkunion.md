# DeploymentConfigNetworkUnion


## Supported Types

### `models.DeploymentConfigNetworkUseDefault`

```typescript
const value: models.DeploymentConfigNetworkUseDefault = {
  type: "use-default",
};
```

### `models.DeploymentConfigNetworkCreate`

```typescript
const value: models.DeploymentConfigNetworkCreate = {
  type: "create",
};
```

### `models.DeploymentConfigNetworkByoVpcAws`

```typescript
const value: models.DeploymentConfigNetworkByoVpcAws = {
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

### `models.DeploymentConfigNetworkByoVpcGcp`

```typescript
const value: models.DeploymentConfigNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.DeploymentConfigNetworkByoVnetAzure`

```typescript
const value: models.DeploymentConfigNetworkByoVnetAzure = {
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

