// src/theme.ts
import {createTheme} from '@mui/material/styles';

export type ThemeModeSetting = 'light' | 'dark' | 'system';

const THEME_STORAGE_KEY = 'appThemeMode';

export const loadThemeMode = (): ThemeModeSetting => {
    try {
        const savedTheme = localStorage.getItem(THEME_STORAGE_KEY);
        if (savedTheme === 'light' || savedTheme === 'dark' || savedTheme === 'system') return savedTheme;
    } catch {
        // Storage can be unavailable; fall back to the system theme.
    }
    return 'system';
};

export const saveThemeMode = (themeMode: ThemeModeSetting) => {
    try {
        localStorage.setItem(THEME_STORAGE_KEY, themeMode);
    } catch (error) {
        console.warn('Failed to persist theme mode:', error);
    }
};

export const createAppTheme = (themeMode: ThemeModeSetting, prefersDarkMode: boolean) => {
    const mode: 'light' | 'dark' = themeMode === 'system' ? (prefersDarkMode ? 'dark' : 'light') : themeMode;
    const isDark = mode === 'dark';
    return createTheme({
        palette: {
            mode,
            primary: {main: '#6366f1'},
            success: {main: isDark ? '#34d399' : '#059669'},
            warning: {main: isDark ? '#fbbf24' : '#d97706'},
            background: {
                default: isDark ? '#0b0f17' : '#f4f6fb',
                paper: isDark ? '#121824' : '#ffffff',
            },
            divider: isDark ? 'rgba(148, 163, 184, 0.16)' : 'rgba(15, 23, 42, 0.10)',
        },
        shape: {borderRadius: 12},
        typography: {
            fontFamily: 'Inter, "Segoe UI", system-ui, sans-serif',
            h5: {fontWeight: 720, letterSpacing: '-0.025em'},
            h6: {fontWeight: 700, letterSpacing: '-0.015em'},
            button: {fontWeight: 650, letterSpacing: 0, textTransform: 'none'},
        },
        components: {
            MuiButton: {
                defaultProps: {disableElevation: true},
                styleOverrides: {root: {borderRadius: 10, minHeight: 38}},
            },
            MuiCard: {
                styleOverrides: {
                    root: {
                        borderRadius: 18,
                        boxShadow: isDark
                            ? '0 18px 50px rgba(0, 0, 0, 0.28)'
                            : '0 18px 50px rgba(30, 41, 59, 0.07)',
                    },
                },
            },
            MuiFormControl: {
                styleOverrides: {root: {'& .MuiOutlinedInput-root': {borderRadius: 10}}},
            },
            MuiTooltip: {defaultProps: {arrow: true}},
        },
    });
};
