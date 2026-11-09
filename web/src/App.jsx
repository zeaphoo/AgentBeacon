import { Navigate, Route, Routes } from 'react-router-dom'
import Layout from './components/Layout'
import PreferencesSync from './components/PreferencesSync'
import Control from './pages/Control'
import Agents from './pages/Agents'
import Dashboard from './pages/Dashboard'
import Settings from './pages/Settings'

export default function App() {
  return (
    <>
      <PreferencesSync />
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<Dashboard />} />
          <Route path="/control" element={<Control />} />
          <Route path="/agents" element={<Agents />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </>
  )
}
