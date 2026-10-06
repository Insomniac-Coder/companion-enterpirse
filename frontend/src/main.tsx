import React from 'react';
import { createRoot } from 'react-dom/client';
import SignInGate from './SignInGate';
import './workbench.css';
import './pages.css';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <SignInGate />
  </React.StrictMode>,
);
