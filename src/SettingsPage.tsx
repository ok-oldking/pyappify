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

    // Props for Pip Cache Directory
    currentPipCacheDir: string;
    pipCacheDirOptions: string[];
    onChangePipCacheDir: (value: string) => void;

    onBack: () => void;
}

const SettingsPage: React.FC<SettingsPageProps> = ({
                                                       currentTheme,
                                                       onChangeTheme,
                                                       currentPipCacheDir,
                                                       pipCacheDirOptions,
                                                       onChangePipCacheDir,
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

    return (
        <Container maxWidth="sm" sx={{py: 4}}>
            <Paper elevation={3} sx={{p: 3}}>
                <Typography variant="h4" component="h1" gutterBottom sx={{textAlign: 'center', mb: 3}}>
                    {t('Settings')}
                </Typography>

                {/* Language Setting */}
                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="language-select-label">{t('Language')}</InputLabel>
                        <Select
                            labelId="language-select-label"
                            id="language-select"
                            value={i18n.language.startsWith('zh') ? 'zh' : 'en'}
                            label={t('Language')}
                            onChange={handleLanguageChange}
                        >
                            <MenuItem value="en">{t('English')}</MenuItem>
                            <MenuItem value="zh">{t('Chinese')}</MenuItem>
                        </Select>
                    </FormControl>
                </Box>

                {/* Theme Setting */}
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

                {/* Pip Cache Directory Setting */}
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
                                    {option}
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