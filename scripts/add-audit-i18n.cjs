// 一次性脚本：审计日志（P2）新增 i18n key（10 语言同步）
// 运行：node scripts/add-audit-i18n.cjs
const fs = require('fs')
const path = require('path')

const DIR = path.join(__dirname, '..', 'src', 'i18n')
const LANGS = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'fr', 'de', 'ru', 'pt', 'ar']

// op 标签（13 个操作类型）
const OPS = [
  'port_kill', 'service_action', 'firewall_add', 'firewall_remove', 'firewall_toggle',
  'docker_container_action', 'docker_container_remove', 'docker_image_remove',
  'site_toggle', 'site_delete', 'server_reboot', 'software_action', 'custom_software_action',
  'tunnel_create', 'tunnel_close', 'tunnel_delete', 'tunnel_restore',
]

// 每种语言的 audit 段（title/empty/clear/clearConfirm/success/error + op.* 标签）
const T = {
  'zh-CN': {
    title: '操作日志', empty: '暂无操作记录', clear: '清空', clearConfirm: '确认清空全部日志？',
    success: '成功', error: '失败',
    op: {
      port_kill: '结束进程', service_action: '服务操作', firewall_add: '防火墙-添加', firewall_remove: '防火墙-删除',
      firewall_toggle: '防火墙-开关', docker_container_action: 'Docker-容器操作', docker_container_remove: 'Docker-删除容器',
      docker_image_remove: 'Docker-删除镜像', site_toggle: '站点-启停', site_delete: '删除站点',
      server_reboot: '重启服务器', software_action: '软件操作', custom_software_action: '自定义软件操作',
      tunnel_restore: 'نفق استعادة',
      tunnel_delete: 'نفق حذف',
      tunnel_close: 'نفق إغلاق',
      tunnel_create: 'نفق إنشاء',
      tunnel_restore: 'Túnel restaurar',
      tunnel_delete: 'Túnel excluir',
      tunnel_close: 'Túnel fechar',
      tunnel_create: 'Túnel criar',
      tunnel_restore: 'Туннель восстановить',
      tunnel_delete: 'Туннель удалить',
      tunnel_close: 'Туннель закрыть',
      tunnel_create: 'Туннель создать',
      tunnel_restore: 'Tunnel wiederherstellen',
      tunnel_delete: 'Tunnel löschen',
      tunnel_close: 'Tunnel schließen',
      tunnel_create: 'Tunnel erstellen',
      tunnel_restore: 'Tunnel restaurer',
      tunnel_delete: 'Tunnel supprimer',
      tunnel_close: 'Tunnel fermer',
      tunnel_create: 'Tunnel créer',
      tunnel_restore: '터널-복원',
      tunnel_delete: '터널-삭제',
      tunnel_close: '터널-닫기',
      tunnel_create: '터널-생성',
      tunnel_restore: 'トンネル-復元',
      tunnel_delete: 'トンネル-削除',
      tunnel_close: 'トンネル-閉じる',
      tunnel_create: 'トンネル-作成',
      tunnel_restore: 'Tunnel restore',
      tunnel_delete: 'Tunnel delete',
      tunnel_close: 'Tunnel close',
      tunnel_create: 'Tunnel create',
      tunnel_restore: '隧道-恢復',
      tunnel_delete: '隧道-刪除',
      tunnel_close: '隧道-關閉',
      tunnel_create: '隧道-建立',
      tunnel_restore: '隧道-恢复',
      tunnel_delete: '隧道-删除',
      tunnel_close: '隧道-关闭',
      tunnel_create: '隧道-创建',
    },
  },
  'zh-TW': {
    title: '操作日誌', empty: '暫無操作記錄', clear: '清空', clearConfirm: '確認清空全部日誌？',
    success: '成功', error: '失敗',
    op: {
      port_kill: '結束進程', service_action: '服務操作', firewall_add: '防火牆-新增', firewall_remove: '防火牆-刪除',
      firewall_toggle: '防火牆-開關', docker_container_action: 'Docker-容器操作', docker_container_remove: 'Docker-刪除容器',
      docker_image_remove: 'Docker-刪除鏡像', site_toggle: '站點-啟停', site_delete: '刪除站點',
      server_reboot: '重啟伺服器', software_action: '軟體操作', custom_software_action: '自訂軟體操作',
    },
  },
  en: {
    title: 'Audit log', empty: 'No audit records yet', clear: 'Clear', clearConfirm: 'Clear all logs?',
    success: 'Success', error: 'Failed',
    op: {
      port_kill: 'Kill process', service_action: 'Service action', firewall_add: 'Firewall add', firewall_remove: 'Firewall remove',
      firewall_toggle: 'Firewall toggle', docker_container_action: 'Docker container action', docker_container_remove: 'Docker remove container',
      docker_image_remove: 'Docker remove image', site_toggle: 'Site enable/disable', site_delete: 'Delete site',
      server_reboot: 'Reboot server', software_action: 'Software action', custom_software_action: 'Custom software action',
    },
  },
  ja: {
    title: '操作ログ', empty: '操作記録はまだありません', clear: 'クリア', clearConfirm: 'すべてのログをクリアしますか？',
    success: '成功', error: '失敗',
    op: {
      port_kill: 'プロセス終了', service_action: 'サービス操作', firewall_add: 'ファイアウォール-追加', firewall_remove: 'ファイアウォール-削除',
      firewall_toggle: 'ファイアウォール-切替', docker_container_action: 'Docker-コンテナ操作', docker_container_remove: 'Docker-コンテナ削除',
      docker_image_remove: 'Docker-イメージ削除', site_toggle: 'サイト-有効/無効', site_delete: 'サイト削除',
      server_reboot: 'サーバー再起動', software_action: 'ソフトウェア操作', custom_software_action: 'カスタムソフト操作',
    },
  },
  ko: {
    title: '작업 로그', empty: '작업 기록이 없습니다', clear: '비우기', clearConfirm: '모든 로그를 비우시겠습니까?',
    success: '성공', error: '실패',
    op: {
      port_kill: '프로세스 종료', service_action: '서비스 작업', firewall_add: '방화벽-추가', firewall_remove: '방화벽-삭제',
      firewall_toggle: '방화벽-전환', docker_container_action: 'Docker-컨테이너 작업', docker_container_remove: 'Docker-컨테이너 삭제',
      docker_image_remove: 'Docker-이미지 삭제', site_toggle: '사이트-활성/비활성', site_delete: '사이트 삭제',
      server_reboot: '서버 재부팅', software_action: '소프트웨어 작업', custom_software_action: '사용자 정의 소프트웨어 작업',
    },
  },
  fr: {
    title: "Journal d'audit", empty: "Aucun enregistrement d'audit", clear: 'Effacer', clearConfirm: 'Effacer tous les journaux ?',
    success: 'Succès', error: 'Échec',
    op: {
      port_kill: 'Tuer le processus', service_action: 'Action service', firewall_add: 'Pare-feu ajouter', firewall_remove: 'Pare-feu supprimer',
      firewall_toggle: 'Pare-feu basculer', docker_container_action: 'Docker action conteneur', docker_container_remove: 'Docker supprimer conteneur',
      docker_image_remove: 'Docker supprimer image', site_toggle: 'Site activer/désactiver', site_delete: 'Supprimer le site',
      server_reboot: 'Redémarrer le serveur', software_action: 'Action logiciel', custom_software_action: 'Action logiciel personnalisé',
    },
  },
  de: {
    title: 'Prüfprotokoll', empty: 'Noch keine Prüfeinträge', clear: 'Leeren', clearConfirm: 'Alle Protokolle leeren?',
    success: 'Erfolg', error: 'Fehlgeschlagen',
    op: {
      port_kill: 'Prozess beenden', service_action: 'Dienstaktion', firewall_add: 'Firewall hinzufügen', firewall_remove: 'Firewall entfernen',
      firewall_toggle: 'Firewall umschalten', docker_container_action: 'Docker Container-Aktion', docker_container_remove: 'Docker Container entfernen',
      docker_image_remove: 'Docker Image entfernen', site_toggle: 'Site aktivieren/deaktivieren', site_delete: 'Site löschen',
      server_reboot: 'Server neu starten', software_action: 'Software-Aktion', custom_software_action: 'Benutzerdefinierte Software-Aktion',
    },
  },
  ru: {
    title: 'Журнал аудита', empty: 'Записей аудита пока нет', clear: 'Очистить', clearConfirm: 'Очистить все записи?',
    success: 'Успех', error: 'Ошибка',
    op: {
      port_kill: 'Завершить процесс', service_action: 'Действие со службой', firewall_add: 'Файрвол добавить', firewall_remove: 'Файрвол удалить',
      firewall_toggle: 'Файрвол переключить', docker_container_action: 'Docker действие с контейнером', docker_container_remove: 'Docker удалить контейнер',
      docker_image_remove: 'Docker удалить образ', site_toggle: 'Сайт вкл/выкл', site_delete: 'Удалить сайт',
      server_reboot: 'Перезагрузка сервера', software_action: 'Действие с ПО', custom_software_action: 'Действие с кастомным ПО',
    },
  },
  pt: {
    title: 'Registro de auditoria', empty: 'Nenhum registro de auditoria ainda', clear: 'Limpar', clearConfirm: 'Limpar todos os registros?',
    success: 'Sucesso', error: 'Falha',
    op: {
      port_kill: 'Encerrar processo', service_action: 'Ação de serviço', firewall_add: 'Firewall adicionar', firewall_remove: 'Firewall remover',
      firewall_toggle: 'Firewall alternar', docker_container_action: 'Docker ação de contêiner', docker_container_remove: 'Docker remover contêiner',
      docker_image_remove: 'Docker remover imagem', site_toggle: 'Site ativar/desativar', site_delete: 'Excluir site',
      server_reboot: 'Reiniciar servidor', software_action: 'Ação de software', custom_software_action: 'Ação de software personalizado',
    },
  },
  ar: {
    title: 'سجل التدقيق', empty: 'لا توجد سجلات تدقيق بعد', clear: 'مسح', clearConfirm: 'مسح جميع السجلات؟',
    success: 'نجاح', error: 'فشل',
    op: {
      port_kill: 'إنهاء العملية', service_action: 'إجراء الخدمة', firewall_add: 'جدار الحماية إضافة', firewall_remove: 'جدار الحماية حذف',
      firewall_toggle: 'جدار الحماية تبديل', docker_container_action: 'Docker إجراء الحاوية', docker_container_remove: 'Docker حذف الحاوية',
      docker_image_remove: 'Docker حذف الصورة', site_toggle: 'الموقع تمكين/تعطيل', site_delete: 'حذف الموقع',
      server_reboot: 'إعادة تشغيل الخادم', software_action: 'إجراء البرنامج', custom_software_action: 'إجراء برنامج مخصص',
    },
  },
}

let changed = 0
for (const lang of LANGS) {
  const file = path.join(DIR, `${lang}.json`)
  const data = JSON.parse(fs.readFileSync(file, 'utf8'))
  const t = T[lang]
  if (!data.audit) data.audit = {}
  for (const [k, v] of Object.entries(t)) {
    if (k === 'op') {
      if (!data.audit.op) data.audit.op = {}
      for (const op of OPS) {
        if (!(op in data.audit.op)) {
          data.audit.op[op] = t.op[op]
          changed++
        }
      }
    } else if (!(k in data.audit)) {
      data.audit[k] = v
      changed++
    }
  }
  fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n', 'utf8')
}
console.log(`Added ${changed} audit keys across ${LANGS.length} languages`)
