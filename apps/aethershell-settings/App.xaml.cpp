#include "pch.h"
#include "App.xaml.h"
#include "MainWindow.xaml.h"
#if __has_include("XamlTypeInfo.Impl.g.cpp")
#include "XamlTypeInfo.Impl.g.cpp"
#endif
#if __has_include("XamlTypeInfo.g.cpp")
#include "XamlTypeInfo.g.cpp"
#endif
#if __has_include("XamlMetaDataProvider.g.cpp")
#include "XamlMetaDataProvider.g.cpp"
#endif
#if __has_include("module.g.cpp")
#include "module.g.cpp"
#endif

using namespace winrt;
using namespace Microsoft::UI::Xaml;

namespace winrt::AetherShell::Settings::implementation
{
    App::App()
    {
        InitializeComponent();
    }

    void App::OnLaunched(Microsoft::UI::Xaml::LaunchActivatedEventArgs const&)
    {
        m_window = make<MainWindow>();
        m_window.Activate();
    }
}
