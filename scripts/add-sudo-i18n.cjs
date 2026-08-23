// 一次性脚本：权限模型 v8 新增 i18n key（10 语言同步）
// 运行：node scripts/add-sudo-i18n.cjs
const fs = require('fs')
const path = require('path')

const DIR = path.join(__dirname, '..', 'src', 'i18n')
const LANGS = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'fr', 'de', 'ru', 'pt', 'ar']

// 各语言翻译表
const T = {
  'zh-CN': {
    sidebar: {
      authMode: '连接模式', authModeRoot: 'Root 直连', authModeSudo: '普通用户 + Sudo',
      sudoPasswordMode: 'Sudo 密码策略', sudoPasswordAsk: '每次输入', sudoPasswordKeyring: '保存到钥匙串',
      sudoPassword: 'Sudo 密码', enterSudoPassword: '输入 Sudo 密码',
    },
    quickCommands: {
      highRisk: '高风险命令', highRiskHint: '执行前需二次确认',
      highRiskConfirm: '该命令已标记为高风险，确认执行？\n{{command}}',
    },
    sudoDialog: {
      title: '需要 Sudo 密码',
      description: '此操作需要管理权限。请输入 Sudo 密码（仅本次会话使用）。',
      incorrect: 'Sudo 密码不正确，请重试。', password: 'Sudo 密码', passwordPlaceholder: '输入 Sudo 密码',
    },
  },
  'zh-TW': {
    sidebar: {
      authMode: '連接模式', authModeRoot: 'Root 直連', authModeSudo: '普通用戶 + Sudo',
      sudoPasswordMode: 'Sudo 密碼策略', sudoPasswordAsk: '每次輸入', sudoPasswordKeyring: '保存到鑰匙圈',
      sudoPassword: 'Sudo 密碼', enterSudoPassword: '輸入 Sudo 密碼',
    },
    quickCommands: {
      highRisk: '高風險命令', highRiskHint: '執行前需二次確認',
      highRiskConfirm: '該命令已標記為高風險，確認執行？\n{{command}}',
    },
    sudoDialog: {
      title: '需要 Sudo 密碼',
      description: '此操作需要管理權限。請輸入 Sudo 密碼（僅本次會話使用）。',
      incorrect: 'Sudo 密碼不正確，請重試。', password: 'Sudo 密碼', passwordPlaceholder: '輸入 Sudo 密碼',
    },
  },
  en: {
    sidebar: {
      authMode: 'Connection mode', authModeRoot: 'Root direct', authModeSudo: 'User + Sudo',
      sudoPasswordMode: 'Sudo password policy', sudoPasswordAsk: 'Ask each time', sudoPasswordKeyring: 'Save to keyring',
      sudoPassword: 'Sudo password', enterSudoPassword: 'Enter Sudo password',
    },
    quickCommands: {
      highRisk: 'High-risk command', highRiskHint: 'Requires confirmation before running',
      highRiskConfirm: 'This command is marked as high-risk. Run it?\n{{command}}',
    },
    sudoDialog: {
      title: 'Sudo password required',
      description: 'This operation requires admin privileges. Enter the Sudo password (used for this session only).',
      incorrect: 'Incorrect Sudo password, please try again.', password: 'Sudo password', passwordPlaceholder: 'Enter Sudo password',
    },
  },
  ja: {
    sidebar: {
      authMode: '接続モード', authModeRoot: 'Root 直接接続', authModeSudo: '一般ユーザー + Sudo',
      sudoPasswordMode: 'Sudo パスワードポリシー', sudoPasswordAsk: '毎回入力', sudoPasswordKeyring: 'キーリングに保存',
      sudoPassword: 'Sudo パスワード', enterSudoPassword: 'Sudo パスワードを入力',
    },
    quickCommands: {
      highRisk: '高リスクコマンド', highRiskHint: '実行前に確認が必要',
      highRiskConfirm: 'このコマンドは高リスクに設定されています。実行しますか？\n{{command}}',
    },
    sudoDialog: {
      title: 'Sudo パスワードが必要です',
      description: 'この操作には管理者権限が必要です。Sudo パスワードを入力してください（このセッションのみ）。',
      incorrect: 'Sudo パスワードが正しくありません。もう一度お試しください。', password: 'Sudo パスワード', passwordPlaceholder: 'Sudo パスワードを入力',
    },
  },
  ko: {
    sidebar: {
      authMode: '연결 모드', authModeRoot: 'Root 직접 연결', authModeSudo: '일반 사용자 + Sudo',
      sudoPasswordMode: 'Sudo 비밀번호 정책', sudoPasswordAsk: '매번 입력', sudoPasswordKeyring: '키링에 저장',
      sudoPassword: 'Sudo 비밀번호', enterSudoPassword: 'Sudo 비밀번호 입력',
    },
    quickCommands: {
      highRisk: '고위험 명령', highRiskHint: '실행 전 확인 필요',
      highRiskConfirm: '이 명령은 고위험으로 표시되었습니다. 실행하시겠습니까?\n{{command}}',
    },
    sudoDialog: {
      title: 'Sudo 비밀번호 필요',
      description: '이 작업에는 관리자 권한이 필요합니다. Sudo 비밀번호를 입력하세요(이 세션에서만 사용).',
      incorrect: 'Sudo 비밀번호가 올바르지 않습니다. 다시 시도하세요.', password: 'Sudo 비밀번호', passwordPlaceholder: 'Sudo 비밀번호 입력',
    },
  },
  fr: {
    sidebar: {
      authMode: 'Mode de connexion', authModeRoot: 'Root direct', authModeSudo: 'Utilisateur + Sudo',
      sudoPasswordMode: 'Politique de mot de passe Sudo', sudoPasswordAsk: 'Demander à chaque fois', sudoPasswordKeyring: 'Enregistrer dans le trousseau',
      sudoPassword: 'Mot de passe Sudo', enterSudoPassword: 'Entrez le mot de passe Sudo',
    },
    quickCommands: {
      highRisk: 'Commande à haut risque', highRiskHint: 'Confirmation requise avant exécution',
      highRiskConfirm: 'Cette commande est marquée à haut risque. L\'exécuter ?\n{{command}}',
    },
    sudoDialog: {
      title: 'Mot de passe Sudo requis',
      description: 'Cette opération nécessite des privilèges admin. Entrez le mot de passe Sudo (utilisé uniquement pour cette session).',
      incorrect: 'Mot de passe Sudo incorrect, veuillez réessayer.', password: 'Mot de passe Sudo', passwordPlaceholder: 'Entrez le mot de passe Sudo',
    },
  },
  de: {
    sidebar: {
      authMode: 'Verbindungsmodus', authModeRoot: 'Root direkt', authModeSudo: 'Benutzer + Sudo',
      sudoPasswordMode: 'Sudo-Passwortrichtlinie', sudoPasswordAsk: 'Jedes Mal fragen', sudoPasswordKeyring: 'Im Schlüsselbund speichern',
      sudoPassword: 'Sudo-Passwort', enterSudoPassword: 'Sudo-Passwort eingeben',
    },
    quickCommands: {
      highRisk: 'Risikoreicher Befehl', highRiskHint: 'Bestätigung vor Ausführung erforderlich',
      highRiskConfirm: 'Dieser Befehl ist als risikoreich markiert. Ausführen?\n{{command}}',
    },
    sudoDialog: {
      title: 'Sudo-Passwort erforderlich',
      description: 'Diese Aktion erfordert Administratorrechte. Geben Sie das Sudo-Passwort ein (nur für diese Sitzung).',
      incorrect: 'Sudo-Passwort falsch, bitte erneut versuchen.', password: 'Sudo-Passwort', passwordPlaceholder: 'Sudo-Passwort eingeben',
    },
  },
  ru: {
    sidebar: {
      authMode: 'Режим подключения', authModeRoot: 'Root напрямую', authModeSudo: 'Пользователь + Sudo',
      sudoPasswordMode: 'Политика пароля Sudo', sudoPasswordAsk: 'Спрашивать каждый раз', sudoPasswordKeyring: 'Сохранить в связку ключей',
      sudoPassword: 'Пароль Sudo', enterSudoPassword: 'Введите пароль Sudo',
    },
    quickCommands: {
      highRisk: 'Команда высокого риска', highRiskHint: 'Требуется подтверждение перед запуском',
      highRiskConfirm: 'Эта команда помечена как высокорисковая. Выполнить?\n{{command}}',
    },
    sudoDialog: {
      title: 'Требуется пароль Sudo',
      description: 'Для этой операции требуются права администратора. Введите пароль Sudo (только для этой сессии).',
      incorrect: 'Неверный пароль Sudo, попробуйте ещё раз.', password: 'Пароль Sudo', passwordPlaceholder: 'Введите пароль Sudo',
    },
  },
  pt: {
    sidebar: {
      authMode: 'Modo de conexão', authModeRoot: 'Root direto', authModeSudo: 'Usuário + Sudo',
      sudoPasswordMode: 'Política de senha Sudo', sudoPasswordAsk: 'Perguntar sempre', sudoPasswordKeyring: 'Salvar no chaveiro',
      sudoPassword: 'Senha Sudo', enterSudoPassword: 'Digite a senha Sudo',
    },
    quickCommands: {
      highRisk: 'Comando de alto risco', highRiskHint: 'Confirmação necessária antes de executar',
      highRiskConfirm: 'Este comando está marcado como de alto risco. Executar?\n{{command}}',
    },
    sudoDialog: {
      title: 'Senha Sudo necessária',
      description: 'Esta operação requer privilégios de administrador. Digite a senha Sudo (usada apenas nesta sessão).',
      incorrect: 'Senha Sudo incorreta, tente novamente.', password: 'Senha Sudo', passwordPlaceholder: 'Digite a senha Sudo',
    },
  },
  ar: {
    sidebar: {
      authMode: 'وضع الاتصال', authModeRoot: 'اتصال مباشر بـ Root', authModeSudo: 'مستخدم + Sudo',
      sudoPasswordMode: 'سياسة كلمة مرور Sudo', sudoPasswordAsk: 'اسأل في كل مرة', sudoPasswordKeyring: 'حفظ في سلسلة المفاتيح',
      sudoPassword: 'كلمة مرور Sudo', enterSudoPassword: 'أدخل كلمة مرور Sudo',
    },
    quickCommands: {
      highRisk: 'أمر عالي الخطورة', highRiskHint: 'يتطلب تأكيدًا قبل التشغيل',
      highRiskConfirm: 'تم وضع علامة على هذا الأمر كعالي الخطورة. هل تريد تنفيذه؟\n{{command}}',
    },
    sudoDialog: {
      title: 'كلمة مرور Sudo مطلوبة',
      description: 'تتطلب هذه العملية صلاحيات المدير. أدخل كلمة مرور Sudo (تُستخدم لهذه الجلسة فقط).',
      incorrect: 'كلمة مرور Sudo غير صحيحة، حاول مرة أخرى.', password: 'كلمة مرور Sudo', passwordPlaceholder: 'أدخل كلمة مرور Sudo',
    },
  },
}

const SECTIONS = ['sidebar', 'quickCommands', 'sudoDialog']

let changed = 0
for (const lang of LANGS) {
  const file = path.join(DIR, `${lang}.json`)
  const data = JSON.parse(fs.readFileSync(file, 'utf8'))
  const t = T[lang]
  for (const sec of SECTIONS) {
    if (!data[sec]) data[sec] = {}
    for (const [k, v] of Object.entries(t[sec])) {
      if (!(k in data[sec])) {
        data[sec][k] = v
        changed++
      }
    }
  }
  fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n', 'utf8')
}
console.log(`Added ${changed} keys across ${LANGS.length} languages`)
