// src/i18n.ts
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

const resources = {
    en: {
        translation: {
            "appMadeWith": "App made with {{name}}",
            "defenderExclusionAdded": "Defender exclusion for '{{appName}}' added successfully.",
            "failedToAddExclusion": "Failed to add exclusion: {{errorMessage}}",
        },
    },
    zh: {
        translation: {
            "Confirm Deletion": "确认删除",
            "appMadeWith": "使用 {{name}} 打包",
            "Settings": "设置",
            "Processing action...": "正在处理操作...",
            "Loading app...": "正在加载应用...",
            "No apps found. Add one using the form above.": "未找到应用。请使用上方的表单添加一个。",
            "Update": "升级",
            "Downgrade": "降级",
            "Set": "设置",
            "Check for updates": "检查更新",
            "Update App": "更新应用",
            "Downgrade App": "降级应用",
            "Set Version": "设置版本",
            "(Not Installed)": "(未安装)",
            "(Installing...)": "(正在安装...)",
            "(Running)": "(运行中)",
            "Stop App": "停止应用",
            "Stopping...": "正在停止...",
            "Console": "控制台",
            "Start App": "启动应用",
            "Add Defender Exclusion": "添加Defender白名单",
            "Starting...": "正在启动...",
            "Install": "安装",
            "Installing...": "正在安装...",
            "Delete": "删除",
            "Are you sure you want to delete {{appName}}? This action cannot be undone.": "您确定要删除 {{appName}} 吗？此操作无法撤销。",
            "Deleting...": "正在删除...",
            "Change Profile": "切换配置",
            "Changing...": "正在切换...",
            "Change version...": "更改版本...",
            "(Update)": "(升级)",
            "(Downgrade)": "(降级)",
            "Updating...": "升级中...",
            "Downgrading...": "降级中...",
            "App Refreshed.": "应用已刷新",
            "Refreshing app...":"刷新应用中",
            "Setting...": "设置中...",
            "App is marked installed but has no current version. Consider re-installing or setting a version if available.": "应用已标记为已安装但没有当前版本。如果可用，请考虑重新安装或设置一个版本。",
            "No other versions or profiles available for modification.": "没有其他版本或配置可供修改。",
            "No other versions available. You can change the profile.": "没有其他可用版本。您可以更改配置。",

            // UpdateLogPage
            "Back to App": "返回应用",
            "Update Notes for {{appName}} (Version: {{version}})": "{{appName}} (版本: {{version}}) 的升级说明",
            "Downgrade Notes for {{appName}} (Version: {{version}})": "{{appName}} (版本: {{version}}) 的降级说明",
            "Set Notes for {{appName}} (Version: {{version}})": "{{appName}} (版本: {{version}}) 的版本设置说明",
            "Loading notes...": "正在加载说明...",
            "Failed to load notes: {{error}}": "加载说明失败: {{error}}",
            "Cancel": "取消",
            "Confirm {{actionType}}": "确认{{actionType}}",

            // ConsolePage
            "Process in progress...": "进程正在进行中...",
            "Process finished.{{errorText}} Review logs and click Done.": "处理完成。{{errorText}}请查看日志并点击“完成”。",
            " There were errors.": " 存在错误。",
            "Back (Process Running)": "返回 (进程运行中)",
            "Done": "完成",
            "No logs received yet for {{appName}}.": "尚未收到 {{appName}} 的日志。",
            "Installing App: {{appName}}": "正在安装应用: {{appName}}",
            "Starting App: {{appName}}": "正在启动应用: {{appName}}",
            "{{actionType}} App: {{appName}}": "{{actionType}}应用: {{appName}}",
            "Console: {{appName}}": "控制台: {{appName}}",
            "Changing Profile: {{appName}} to '{{newProfile}}'": "正在切换配置: {{appName}} 到 '{{newProfile}}'",

            // SettingsPage
            "Language": "语言",
            "English": "English (英语)",
            "Chinese": "中文 (Chinese)",
            "Theme": "主题",
            "System Default": "跟随系统",
            "Light": "浅色模式",
            "Dark": "深色模式",
            "Pip Cache Directory": "Pip 缓存目录",
            "App Install Directory": "应用安装目录",
            "Pip Index URL": "Pip 镜像源",
            "PyPI": "PyPI (官方)",
            "Tsinghua": "清华大学",
            "Aliyun": "阿里云",
            "USTC": "中国科学技术大学",
            "Huawei Cloud": "华为云",
            "Tencent Cloud": "腾讯云",

            // Profile Chooser
            "Choose Profile for {{appName}}": "为 {{appName}} 选择配置",
            "Profile": "配置",
            "Confirm & Install": "确认并安装",
            "Starting Install...": "开始安装...",
            "Adding...": "添加中...",
            "No profiles available or configured for this app. Please check the app's configuration (ok.yml).": "此应用没有可用或配置的档案。请检查应用的配置 (ok.yml)。",
            "Back": "返回",
            "Please select a profile.": "请选择一个配置。",

            // Change Profile
            "Change Profile for {{appName}}": "为 {{appName}} 更改配置",
            "Current Profile: {{profile}}": "当前配置: {{profile}}",
            "New Profile": "新配置",
            " (Current)": " (当前)",
            "Initiating...": "正在初始化...",
            "Please select a different profile.": "请选择一个不同的配置。",
            "No profiles available for this app. This view should not be reachable in this state.": "此应用没有可用的配置。此视图不应在此状态下可达。",

            // Defender Exclusion
            "defenderExclusionAdded": "已成功为 '{{appName}}' 添加 Defender 白名单。",
            "failedToAddExclusion": "添加白名单失败: {{errorMessage}}",
        }
    }
};

i18n
    .use(LanguageDetector)
    .use(initReactI18next)
    .init({
        resources,
        fallbackLng: "en",
        interpolation: {
            escapeValue: false,
        },
        detection: {
            order: ['localStorage', 'navigator'],
            caches: ['localStorage'],
        },
    });

export default i18n;