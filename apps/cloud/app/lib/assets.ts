// 发布文件命名（docs/decisions.md D38）：每个安装包只有一个文件，Qantrip-<版本>-<系统>-<arm64|x64>[-Setup].<扩展名>。
// 官网与客户端用平台标识引用安装包；固定地址 /dl/<平台标识> 由服务端跳转到最新版本的文件。
export const ASSETS = {
  'macos-arm64': 'macOS-arm64.dmg',
  'macos-x64': 'macOS-x64.dmg',
  'windows-x64': 'Windows-x64-Setup.exe',
  'windows-zip-x64': 'Windows-x64.zip',
  'linux-x64': 'Linux-x64.AppImage',
  'linux-deb-x64': 'Linux-x64.deb',
  'linux-rpm-x64': 'Linux-x64.rpm',
  'linux-arm64': 'Linux-arm64.AppImage',
  'linux-deb-arm64': 'Linux-arm64.deb',
  'linux-rpm-arm64': 'Linux-arm64.rpm',
} as const

export type ClientId = keyof typeof ASSETS

export const isClientId = (id: string): id is ClientId => Object.hasOwn(ASSETS, id)

/** 发布文件名；不知道版本号时省略版本段（只用于显示） */
export const assetName = (id: ClientId, version: string | null) => (version ? `Qantrip-${version}-${ASSETS[id]}` : `Qantrip-${ASSETS[id]}`)

/** 官网的固定下载地址 */
export const downloadPath = (id: ClientId) => `/dl/${id}`
