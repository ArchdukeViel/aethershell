#pragma once
#include "MainWindow.g.h"
#include "MainWindow.xaml.g.h"
#include <winrt/Windows.Data.Json.h>

namespace winrt::AetherShell::Settings::implementation
{
    struct MainWindow : MainWindowT<MainWindow>
    {
        MainWindow();
        void NavView_SelectionChanged(
            winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender,
            winrt::Microsoft::UI::Xaml::Controls::NavigationViewSelectionChangedEventArgs const& args);

        void OnWindowClosed(
            winrt::Windows::Foundation::IInspectable const& sender,
            winrt::Microsoft::UI::Xaml::WindowEventArgs const& args);

    private:
        void NavigateToPage(winrt::hstring const& tag);
        winrt::Microsoft::UI::Xaml::UIElement CreateGeneralView();
        winrt::Microsoft::UI::Xaml::UIElement CreateTaskbarView();
        winrt::Microsoft::UI::Xaml::UIElement CreateDesktopView();
        winrt::Microsoft::UI::Xaml::UIElement CreateWidgetsView();
        winrt::Microsoft::UI::Xaml::UIElement CreateAudioView();
        winrt::Microsoft::UI::Xaml::UIElement CreateScriptsView();

        winrt::Windows::Data::Json::JsonObject m_config{ nullptr };
        bool m_isLoading{ false };

        void LoadConfiguration();
        bool SaveConfiguration();
        void EnsureDefaultConfig();

        winrt::Windows::Data::Json::JsonObject GetOrCreateSection(winrt::hstring const& sectionName);
        void SetSection(winrt::hstring const& sectionName, winrt::Windows::Data::Json::JsonObject const& section);
    };
}

namespace winrt::AetherShell::Settings::factory_implementation
{
    struct MainWindow : MainWindowT<MainWindow, implementation::MainWindow>
    {
    };
}
