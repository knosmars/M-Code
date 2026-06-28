export const translations = {
  en: {
    // General
    'app.title': 'Meyatu Code',
    'common.close': 'Close',
    'common.cancel': 'Cancel',
    'common.save': 'Save',
    'common.delete': 'Delete',
    'common.edit': 'Edit',
    'common.confirm': 'Confirm',
    
    // Chat
    'chat.new': 'New Chat',
    'chat.placeholder': 'Type / for commands',
    'chat.send': 'Send',
    'chat.stop': 'Stop',
    'chat.thinking': 'thinking',
    'chat.tokens': 'tokens',
    
    // Sidebar
    'sidebar.sessions': 'Sessions',
    'sidebar.search': 'Search sessions...',
    'sidebar.noSessions': 'No sessions yet',
    'sidebar.delete': 'Delete session',
    'sidebar.rename': 'Rename',
    
    // Settings
    'settings.title': 'Settings',
    'settings.theme': 'Theme',
    'settings.theme.light': 'Light',
    'settings.theme.dark': 'Dark',
    'settings.theme.system': 'System',
    'settings.fontSize': 'Font Size',
    'settings.showTimestamps': 'Show Timestamps',
    'settings.autoScroll': 'Auto Scroll',
    'settings.hideToolStderr': 'Hide Tool Stderr',
    'settings.showToolCalls': 'Show Tool Calls',
    'settings.language': 'Language',
    'settings.language.en': 'English',
    'settings.language.zh': '中文',
    
    // Details Panel
    'details.title': 'Details',
    'details.session': 'Session',
    'details.provider': 'Provider',
    'details.model': 'Model',
    'details.messages': 'Messages',
    'details.tokenUsage': 'Token usage',
    'details.prompt': 'Prompt',
    'details.completion': 'Completion',
    'details.total': 'Total',
    'details.cost': 'Cost',
    'details.taskPlan': 'Task Plan',
    'details.noSession': 'No active session. Start or select a conversation to see details here.',
    
    // New Session Dialog
    'newSession.title': 'Start New Chat',
    'newSession.workspace': 'Workspace',
    'newSession.provider': 'Provider',
    'newSession.model': 'Model',
    'newSession.create': 'Create Session',
    'newSession.cancel': 'Cancel',
    
    // Command Palette
    'command.title': 'Command Palette',
    'command.search': 'Type a command or search...',
    'command.settings': 'Settings',
    'command.newSession': 'New Session',
    'command.searchSessions': 'Search Sessions',
    
    // Slash Commands
    'slash.fix': 'Fix this',
    'slash.test': 'Write tests for',
    'slash.explain': 'Explain',
    'slash.refactor': 'Refactor',
    'slash.commit': 'Commit changes',
    'slash.review': 'Review code',
    
    // Tool Progress
    'tool.running': 'Running',
    'tool.completed': 'Completed',
    'tool.failed': 'Failed',

    // Composer (batch-1 i18n)
    'composer.offline': 'Offline — cannot send, check your network',
    'composer.workspaceMode': 'Workspace mode',
    'composer.modeLocal': 'Local',
    'composer.modeSshServer': 'Remote (SSH server)',
    'composer.contextLengthTitle': 'Current context length',

    // Chat errors (batch-1 i18n)
    'chat.error.rateLimited': 'Too many requests',
    'chat.error.provider': 'Provider error',
    'chat.error.http': 'Network error',
    'chat.error.keychain': 'Failed to read key',
    'chat.error.notFound': 'Resource not found',
    'chat.error.permissionDenied': 'Permission denied',
    'chat.error.serialization': 'Data parse error',
    'chat.error.internal': 'Internal error',
    'chat.error.noApiKey': 'API key not configured',
    'chat.error.validation': 'Request validation failed',
    'chat.error.sessionLimit': 'Session full',
    'chat.error.ipc': 'Communication error',
    'chat.error.streamTimeout': 'Network timeout, check your connection',
    'chat.error.revertFailed': 'Revert failed',
    'chat.error.attachFailed': 'Attachment read failed',
    'chat.error.retryHint': '(retry later)',
    'chat.newSessionLink': '+ New session',

    // Settings connection (batch-1 i18n)
    'settings.connection.connected': 'Connected',
    'settings.connection.unreachable': 'Unreachable — is the local service running? (e.g. ollama serve)',
    'settings.connection.unchecked': 'Not checked',
    'settings.connection.testTitle': 'Test connection and refresh models',
    'settings.connection.test': 'Test connection',
    'settings.localNoKey': 'Local service (no API key)',

    // Git menu (batch 2a)
    'git.clickLogin': 'Click to sign in to your remote repo',
    'git.loggingIn': 'Signing in…',
    'git.notLoggedIn': 'Not signed in',
    'git.loginFailed': 'GitHub sign-in failed: {msg}',

    // SSH menu (batch 2a)
    'ssh.connected': 'Connected to {host} — click to disconnect',
    'ssh.disconnect': 'Disconnect',
    'ssh.clickConnect': 'Click to connect to a remote host',
    'ssh.disconnected': 'Not connected',
    'ssh.port': 'Port',
    'ssh.username': 'Username',
    'ssh.connect': 'Connect',
    'ssh.authPassword': 'Password',
    'ssh.authKey': 'Key',
    'ssh.passwordPlaceholder': 'Password / key',
    'ssh.keyPathPlaceholder': 'Key file path (~/.ssh/id_rsa)',
    'ssh.pickFile': 'Browse',
    'ssh.cmd.connTest': 'Connection test',
    'ssh.cmd.sysInfo': 'System info',
    'ssh.cmd.diskUsage': 'Disk usage',
    'ssh.cmd.procList': 'Process list',
    'ssh.cmd.memUsage': 'Memory usage',
    'ssh.cmd.netStatus': 'Network status',
    'ssh.cmd.fileList': 'File list',
    'ssh.cmd.svcStatus': 'Service status',
    'ssh.cmd.userMgmt': 'User management',
    'ssh.cmd.logView': 'View logs',
    'ssh.cmd.envVars': 'Environment variables',
    'ssh.cmd.cronJobs': 'Scheduled tasks',
    'ssh.cmd.fileTransfer': 'File transfer',
    'ssh.cmd.portCheck': 'Port check',
    'ssh.cmd.execCmd': 'Run command',

    // MCP Servers (batch 2a)
    'mcp.title': 'MCP Servers',
    'mcp.descBefore': 'Manage Model Context Protocol servers. Tools are exposed to the AI as ',
    'mcp.descAfter': '.',
    'mcp.empty': 'No MCP servers configured yet.',
    'mcp.toolCount': '{n} tools',
    'mcp.collapse': 'Collapse',
    'mcp.tools': 'Tools',
    'mcp.enable': 'Enable',
    'mcp.remove': 'Remove',
    'mcp.noTools': 'None (not connected or no tools)',
    'mcp.phName': 'Name',
    'mcp.phCommand': 'Command',
    'mcp.phArgs': 'Args, space-separated',
    'mcp.phEnv': 'Env vars, one KEY=VALUE per line',
    'mcp.add': 'Add',

    // Token Dashboard (batch 2a)
    'tokens.current': 'Current session',
    'tokens.input': 'Input',
    'tokens.output': 'Output',
    'tokens.total': 'Total',
    'tokens.cost': 'Cost',
    'tokens.allSessions': 'All sessions ({n})',

    // Semantic Index (batch 2a)
    'semantic.title': 'Semantic index',
    'semantic.desc': 'Embed workspace code into a local vector store (.meyatu/vectors.db) for AI semantic search.',
    'semantic.status': 'Status',
    'semantic.indexed': 'Indexed',
    'semantic.notIndexed': 'Not indexed',
    'semantic.loading': 'Loading…',
    'semantic.filesChunks': '{files} files · {chunks} chunks',
    'semantic.filesChunksLabel': 'Files / chunks',
    'semantic.embedModel': 'Embedding model',
    'semantic.endpoint': 'Embedding endpoint',
    'semantic.modelLabel': 'Embedding model',
    'semantic.saveConfig': 'Save config',
    'semantic.savedMsg': 'Saved (next index will fully rebuild after a model change)',
    'semantic.autoIndex': 'Auto background index',
    'semantic.autoIndexHint': ' re-index every 5 minutes',
    'semantic.indexing': 'Indexing…',
    'semantic.rebuild': 'Rebuild index',
    'semantic.indexNow': 'Index now',

    // Parallel agents (batch 2b)
    'agents.empty': 'No parallel agents running',
    'agents.headerProgress': 'Parallel agents — {done} / {total} done',
    'agents.cardTitle': 'Parallel agents ({n})',
    'agents.cardCount': '{done} / {total} done',

    // Models / message (batch 2b)
    'models.more': 'More models ({n})',
    'message.rollback': "Roll back this turn's file changes",

    // SSH errors (batch 3)
    'ssh.error.noKeyFile': 'Select an SSH key file',
    'ssh.error.connectFailed': 'Connection failed',

    // Workspace errors (batch 3)
    'workspace.indexFailed': 'Codebase indexing failed; the AI will have no project context',
  },
  
  zh: {
    // General
    'app.title': 'Meyatu Code',
    'common.close': '关闭',
    'common.cancel': '取消',
    'common.save': '保存',
    'common.delete': '删除',
    'common.edit': '编辑',
    'common.confirm': '确认',
    
    // Chat
    'chat.new': '新对话',
    'chat.placeholder': '输入 / 查看命令',
    'chat.send': '发送',
    'chat.stop': '停止',
    'chat.thinking': '思考中',
    'chat.tokens': 'tokens',
    
    // Sidebar
    'sidebar.sessions': '对话列表',
    'sidebar.search': '搜索对话...',
    'sidebar.noSessions': '暂无对话',
    'sidebar.delete': '删除对话',
    'sidebar.rename': '重命名',
    
    // Settings
    'settings.title': '设置',
    'settings.theme': '主题',
    'settings.theme.light': '浅色',
    'settings.theme.dark': '深色',
    'settings.theme.system': '跟随系统',
    'settings.fontSize': '字体大小',
    'settings.showTimestamps': '显示时间戳',
    'settings.autoScroll': '自动滚动',
    'settings.hideToolStderr': '隐藏工具错误输出',
    'settings.showToolCalls': '显示工具调用',
    'settings.language': '语言',
    'settings.language.en': 'English',
    'settings.language.zh': '中文',
    
    // Details Panel
    'details.title': '详情',
    'details.session': '对话',
    'details.provider': '提供商',
    'details.model': '模型',
    'details.messages': '消息数',
    'details.tokenUsage': 'Token 使用',
    'details.prompt': '输入',
    'details.completion': '输出',
    'details.total': '总计',
    'details.cost': '费用',
    'details.taskPlan': '任务计划',
    'details.noSession': '暂无活动对话。开始或选择一个对话以查看详情。',
    
    // New Session Dialog
    'newSession.title': '开始新对话',
    'newSession.workspace': '工作区',
    'newSession.provider': '提供商',
    'newSession.model': '模型',
    'newSession.create': '创建对话',
    'newSession.cancel': '取消',
    
    // Command Palette
    'command.title': '命令面板',
    'command.search': '输入命令或搜索...',
    'command.settings': '设置',
    'command.newSession': '新对话',
    'command.searchSessions': '搜索对话',
    
    // Slash Commands
    'slash.fix': '修复这个',
    'slash.test': '为以下编写测试',
    'slash.explain': '解释',
    'slash.refactor': '重构',
    'slash.commit': '提交更改',
    'slash.review': '审查代码',
    
    // Tool Progress
    'tool.running': '运行中',
    'tool.completed': '已完成',
    'tool.failed': '已失败',

    // Composer (batch-1 i18n)
    'composer.offline': '离线 — 无法发送，请检查网络',
    'composer.workspaceMode': '工作区模式',
    'composer.modeLocal': '本地（Local）',
    'composer.modeSshServer': '远程（SSH server）',
    'composer.contextLengthTitle': '当前上下文长度',

    // Chat errors (batch-1 i18n)
    'chat.error.rateLimited': '请求过于频繁',
    'chat.error.provider': '服务商错误',
    'chat.error.http': '网络错误',
    'chat.error.keychain': '密钥读取失败',
    'chat.error.notFound': '资源未找到',
    'chat.error.permissionDenied': '权限被拒绝',
    'chat.error.serialization': '数据解析错误',
    'chat.error.internal': '内部错误',
    'chat.error.noApiKey': '未配置 API 密钥',
    'chat.error.validation': '请求校验失败',
    'chat.error.sessionLimit': '会话已满',
    'chat.error.ipc': '通信错误',
    'chat.error.streamTimeout': '网络超时，请检查连接',
    'chat.error.revertFailed': '回滚失败',
    'chat.error.attachFailed': '附件读取失败',
    'chat.error.retryHint': '（可稍后重试）',
    'chat.newSessionLink': '+ 新建会话',

    // Settings connection (batch-1 i18n)
    'settings.connection.connected': '已连接',
    'settings.connection.unreachable': '无法连接 — 本地服务是否在运行？（如 ollama serve）',
    'settings.connection.unchecked': '未检测',
    'settings.connection.testTitle': '测试连接并刷新模型',
    'settings.connection.test': '测试连接',
    'settings.localNoKey': '本地服务（无需 API key）',

    // Git menu (batch 2a)
    'git.clickLogin': '点击登录在线仓库',
    'git.loggingIn': '登录中…',
    'git.notLoggedIn': '未登录',
    'git.loginFailed': 'GitHub 登录失败: {msg}',

    // SSH menu (batch 2a)
    'ssh.connected': '已连接到 {host} — 点击断开',
    'ssh.disconnect': '断开',
    'ssh.clickConnect': '点击连接远程主机',
    'ssh.disconnected': '未连接',
    'ssh.port': '端口',
    'ssh.username': '用户名',
    'ssh.connect': '连接',
    'ssh.authPassword': '密码',
    'ssh.authKey': '密钥',
    'ssh.passwordPlaceholder': '密码/密钥',
    'ssh.keyPathPlaceholder': '密钥文件路径 (~/.ssh/id_rsa)',
    'ssh.pickFile': '选择',
    'ssh.cmd.connTest': '连接测试',
    'ssh.cmd.sysInfo': '系统信息',
    'ssh.cmd.diskUsage': '磁盘使用',
    'ssh.cmd.procList': '进程列表',
    'ssh.cmd.memUsage': '内存使用',
    'ssh.cmd.netStatus': '网络状态',
    'ssh.cmd.fileList': '文件列表',
    'ssh.cmd.svcStatus': '服务状态',
    'ssh.cmd.userMgmt': '用户管理',
    'ssh.cmd.logView': '日志查看',
    'ssh.cmd.envVars': '环境变量',
    'ssh.cmd.cronJobs': '计划任务',
    'ssh.cmd.fileTransfer': '文件传输',
    'ssh.cmd.portCheck': '端口检查',
    'ssh.cmd.execCmd': '执行命令',

    // MCP Servers (batch 2a)
    'mcp.title': 'MCP 服务器',
    'mcp.descBefore': '管理 Model Context Protocol 服务器。工具以 ',
    'mcp.descAfter': ' 暴露给 AI。',
    'mcp.empty': '尚未配置 MCP 服务器。',
    'mcp.toolCount': '{n} 工具',
    'mcp.collapse': '收起',
    'mcp.tools': '工具',
    'mcp.enable': '启用',
    'mcp.remove': '删除',
    'mcp.noTools': '无（未连接或无工具）',
    'mcp.phName': '名称 (name)',
    'mcp.phCommand': '命令 (command)',
    'mcp.phArgs': '参数，空格分隔 (args)',
    'mcp.phEnv': '环境变量，每行 KEY=VALUE (env)',
    'mcp.add': '添加',

    // Token Dashboard (batch 2a)
    'tokens.current': '当前会话',
    'tokens.input': '输入',
    'tokens.output': '输出',
    'tokens.total': '总计',
    'tokens.cost': '费用',
    'tokens.allSessions': '全部会话 ({n})',

    // Semantic Index (batch 2a)
    'semantic.title': '语义索引',
    'semantic.desc': '把工作区代码嵌入本地向量库（.meyatu/vectors.db），供 AI 语义检索。',
    'semantic.status': '状态',
    'semantic.indexed': '已索引',
    'semantic.notIndexed': '尚未索引',
    'semantic.loading': '加载中…',
    'semantic.filesChunks': '{files} 文件 · {chunks} 片段',
    'semantic.filesChunksLabel': '文件 / 片段',
    'semantic.embedModel': '嵌入模型',
    'semantic.endpoint': '嵌入端点',
    'semantic.modelLabel': '嵌入模型',
    'semantic.saveConfig': '保存配置',
    'semantic.savedMsg': '已保存（改模型后下次「索引」将全量重建）',
    'semantic.autoIndex': '自动后台索引',
    'semantic.autoIndexHint': ' 每 5 分钟自动重索引',
    'semantic.indexing': '索引中…',
    'semantic.rebuild': '重建索引',
    'semantic.indexNow': '立即索引',

    // Parallel agents (batch 2b)
    'agents.empty': '无并行 agent',
    'agents.headerProgress': '并行 agent — {done} / {total} 完成',
    'agents.cardTitle': '并行 agent ({n})',
    'agents.cardCount': '{done} / {total} 完成',

    // Models / message (batch 2b)
    'models.more': '更多模型（{n}）',
    'message.rollback': '回滚此轮文件改动',

    // SSH errors (batch 3)
    'ssh.error.noKeyFile': '请选择 SSH 密钥文件',
    'ssh.error.connectFailed': '连接失败',

    // Workspace errors (batch 3)
    'workspace.indexFailed': '代码库索引失败，AI 将无项目上下文',
  },
} as const;

export type TranslationKey = keyof typeof translations.en;
export type Language = 'en' | 'zh';
