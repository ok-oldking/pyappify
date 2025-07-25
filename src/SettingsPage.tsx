// src/components/SettingsPage.tsx
import React from 'react';
import {
    Box,
    Button,
    Container,
    FormControl,
    InputLabel,
    MenuItem,
    Paper,
    Select,
    SelectChangeEvent,
    Typography
} from '@mui/material';
import i18n from "i18next";
import {useTranslation} from 'react-i18next';

type ThemeModeSetting = 'light' | 'dark' | 'system';

interface SettingsPageProps {
    currentTheme: ThemeModeSetting;
    onChangeTheme: (theme: ThemeModeSetting) => void;

    currentPipCacheDir: string;
    pipCacheDirOptions: string[];
    onChangePipCacheDir: (value: string) => void;

    currentIndexUrl: string;
    pipIndexUrlOptions: string[];
    onChangePipIndexUrl: (value: string) => void;

    onBack: () => void;
}

const lang = i18n.language;
const supportedLangs = ['en', 'zh-CN', 'zh-TW', 'es', 'ja', 'kr'];
if (lang.startsWith('zh') && lang !== 'zh-TW' && lang !== 'zh-HK') {
    i18n.changeLanguage('zh-CN');
} else if (lang === 'zh-HK') {
    i18n.changeLanguage('zh-TW');
} else if (!supportedLangs.includes(lang)) {
    i18n.changeLanguage('en');
}

const SettingsPage: React.FC<SettingsPageProps> = ({
                                                       currentTheme,
                                                       onChangeTheme,
                                                       currentPipCacheDir = '',
                                                       pipCacheDirOptions = [],
                                                       onChangePipCacheDir,
                                                       currentIndexUrl = '',
                                                       pipIndexUrlOptions = [],
                                                       onChangePipIndexUrl,
                                                       onBack
                                                   }) => {
    const {t} = useTranslation();

    const handleLanguageChange = (event: SelectChangeEvent<string>) => {
        i18n.changeLanguage(event.target.value);
    };

    const handleThemeChange = (event: SelectChangeEvent<ThemeModeSetting>) => {
        onChangeTheme(event.target.value as ThemeModeSetting);
    };

    const handlePipCacheDirChange = (event: SelectChangeEvent<string>) => {
        onChangePipCacheDir(event.target.value as string);
    };

    const handlePipIndexUrlChange = (event: SelectChangeEvent<string>) => {
        onChangePipIndexUrl(event.target.value as string);
    };

    const getPipIndexUrlName = (url: string) => {
        if (url === '') return t('System Default');
        if (url.includes('pypi.org')) return 'PyPI';
        if (url.includes('tsinghua')) return 'Tsinghua';
        if (url.includes('aliyun')) return 'Aliyun';
        if (url.includes('ustc')) return 'USTC';
        if (url.includes('huaweicloud')) return 'Huawei Cloud';
        if (url.includes('tencent')) return 'Tencent Cloud';
        return url;
    };

    return (
        <Container maxWidth="sm" sx={{py: 4}}>
            <Paper elevation={3} sx={{p: 3}}>
                <Typography variant="h4" component="h1" gutterBottom sx={{textAlign: 'center', mb: 3}}>
                    {t('Settings')}
                </Typography>

                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="language-select-label">{t('Language')}</InputLabel>
                        <Select
                            labelId="language-select-label"
                            id="language-select"
                            value={i18n.language}
                            label={t('Language')}
                            onChange={handleLanguageChange}
                        >
                            <MenuItem value="en">English</MenuItem>
                            <MenuItem value="zh-CN">简体中文</MenuItem>
                            <MenuItem value="zh-TW">繁體中文</MenuItem>
                            <MenuItem value="es">Español</MenuItem>
                            <MenuItem value="ja">日本語</MenuItem>
                            <MenuItem value="kr">한국인</MenuItem>
                        </Select>
                    </FormControl>
                </Box>

                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="theme-select-label">{t('Theme')}</InputLabel>
                        <Select
                            labelId="theme-select-label"
                            id="theme-select"
                            value={currentTheme}
                            label={t('Theme')}
                            onChange={handleThemeChange}
                        >
                            <MenuItem value="system">{t('System Default')}</MenuItem>
                            <MenuItem value="light">{t('Light')}</MenuItem>
                            <MenuItem value="dark">{t('Dark')}</MenuItem>
                        </Select>
                    </FormControl>
                </Box>

                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="pip-cache-dir-select-label">{t('Pip Cache Directory')}</InputLabel>
                        <Select
                            labelId="pip-cache-dir-select-label"
                            id="pip-cache-dir-select"
                            value={currentPipCacheDir}
                            label={t('Pip Cache Directory')}
                            onChange={handlePipCacheDirChange}
                        >
                            {pipCacheDirOptions.map((option) => (
                                <MenuItem key={option} value={option}>
                                    {t(option)}
                                </MenuItem>
                            ))}
                        </Select>
                    </FormControl>
                </Box>

                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="pip-index-url-select-label">{t('Pip Index URL')}</InputLabel>
                        <Select
                            labelId="pip-index-url-select-label"
                            id="pip-index-url-select"
                            value={currentIndexUrl}
                            label={t('Pip Index URL')}
                            onChange={handlePipIndexUrlChange}
                        >
                            {pipIndexUrlOptions.map((option) => (
                                <MenuItem key={option} value={option}>
                                    {t(getPipIndexUrlName(option))}
                                </MenuItem>
                            ))}
                        </Select>
                    </FormControl>
                </Box>

                <Box sx={{mt: 4, display: 'flex', justifyContent: 'center'}}>
                    <Button variant="outlined" onClick={onBack}>
                        {t('Back to App')}
                    </Button>
                </Box>
            </Paper>
        </Container>
    );
};

export default SettingsPage;
